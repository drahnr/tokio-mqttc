use tokio_io::{AsyncRead, AsyncWrite};
use futures::{Poll, Async, AsyncSink, Sink, Future, Stream};
use futures::sync::oneshot::Sender;
use futures::unsync::mpsc::UnboundedReceiver;
use futures_mutex::{FutMutex, FutMutexGuard};

use persistence::Persistence;
use proto::{MqttPacket, QualityOfService, ConnectReturnCode, ConnectAckFlags};
use errors::{Error, Result, ErrorKind};
use super::mqtt_loop::LoopData;
use super::outbound_handlers::RequestHandler;
use super::{ClientReturn, LoopRequest, MqttFramedWriter};


enum State<'p> {
    Receiving,
    Sending(MqttPacket, Option<Sender<Result<ClientReturn>>>),
    Writing(MqttPacket, Option<Sender<Result<ClientReturn>>>),
    Processing(RequestHandler<'p>),
}

/// Handling the connect sequence
/// 
pub struct Connect<'p, I, P> where
    I: AsyncRead + AsyncWrite + Send,
    P: 'p + Send + Persistence,
    'p: 'static,
{
    state: Option<State<'p>>,
    req_queue: UnboundedReceiver<LoopRequest>,
    data_lock: FutMutex<LoopData<'p, P>>,
    writer: MqttFramedWriter<I>,
}


impl<'p, I, P> Connect<'p, I, P>
where
    I: AsyncRead + AsyncWrite + Send,
        P: 'p + Send + Persistence,
        'p: 'static
{
    pub fn new(
        req_queue: UnboundedReceiver<LoopRequest>,
        writer: MqttFramedWriter<I>,
        data_lock: FutMutex<LoopData<'p, P>>,
    ) -> Connect<'p, I, P> {

        Connect {
            state: Some(State::Receiving),
            req_queue,
            data_lock,
            writer,
        }
    }


    fn process_conn_ack(
        mut data: FutMutexGuard<LoopData<'p, P>>,
        packet: MqttPacket,
    ) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        // Check if we are awaiting a CONNACK
        let (_, client) = match data.one_time.entry(OneTimeKey::Connect) {
            Entry::Vacant(v) => {
                return Err(SourceError::ProcessResponse(
                    ErrorKind::from(
                        ProtoErrorKind::UnexpectedResponse(packet.ty.clone()),
                    ).into(),
                ));
            }
            Entry::Occupied(o) => o.remove(),
        };
        // Validate connect return code
        let crc = packet.headers.get::<ConnectReturnCode>().unwrap();
        if crc.is_ok() {
            // If session is not clean, setup a stream.
            let sess = packet.headers.get::<ConnectAckFlags>().unwrap();
            if sess.is_clean() {
                let _ = client.send(Ok(ClientReturn::Onetime(Some(packet))));
            } else {
                let (tx, rx) = unbounded::<SubItem>();
                let custom_rx = rx.map_err(|_| Error::from(ErrorKind::LoopCommsError));
                data.session_subs = Some(tx);
                let _ = client.send(Ok(ClientReturn::Ongoing(
                    vec![Ok((custom_rx.boxed(), QualityOfService::QoS0))],
                )));
            }
            Ok(None)
        } else {
            return Err(SourceError::ProcessResponse(
                ErrorKind::from(ProtoErrorKind::ConnectionRefused(*crc))
                    .into(),
            ));
        }
    }

}

impl<'p, I, P> Future for Connect<'p, I, P>
where
    I: AsyncRead + AsyncWrite + Send,
    P: 'p + Send + Persistence,
    'p: 'static,
{
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
       use self::State::*;

        loop {
            match self.state {
                Some(Receiving) => {
                    let req = match self.req_queue.poll() {
                        Ok(Async::Ready(Some(r))) => r,
                        Ok(Async::Ready(None)) => return Err(ErrorKind::LoopAbortError.into()),
                        Ok(Async::NotReady) => return Ok(Async::NotReady),
                        Err(_) => return Err(ErrorKind::LoopCommsError.into()),
                    };

                    match req {
                        LoopRequest::Internal(p) => self.state = Some(Sending(p, None)),
                        LoopRequest::External(p, ret) => self.state = Some(Sending(p, Some(ret))),
                    }
                }
                Some(Sending(_, _)) => {
                    let (packet, oret) = match self.state.take() {
                        Some(Sending(packet, oret)) => (packet, oret),
                        Some(_) => unreachable!(),
                        None => return Err(ErrorKind::LoopStateError.into()),
                    };
                    let c = packet.clone();
                    match self.writer.start_send(c) {
                        Ok(AsyncSink::Ready) => self.state = Some(Writing(packet, oret)),
                        Ok(AsyncSink::NotReady(_)) => {
                            self.state = Some(Sending(packet, oret));
                            return Ok(Async::NotReady);
                        }
                        Err(e) => {
                            match e {
                                Error(ErrorKind::PacketEncodingError, _) => {
                                    let _ = oret.map(|c| c.send(Err(e)));
                                    self.state = Some(Receiving);
                                }
                                _ => return Err(e),
                            }
                        }
                    };
                }
                Some(Writing(_, _)) => {
                    let (packet, oret) = match self.state.take() {
                        Some(Writing(packet, oret)) => (packet, oret),
                        Some(_) => unreachable!(),
                        None => return Err(ErrorKind::LoopStateError.into()),
                    };

                    let _ = try_ready!(self.writer.poll_complete());

                    self.state = oret.map(|ret| {
                        Processing(RequestHandler::new((packet, ret), self.data_lock.clone()))
                    }).or(Some(Receiving));
                }
                Some(Processing(_)) => {
                    let mut handler = match self.state.take() {
                        Some(Processing(h)) => h,
                        _ => unreachable!(),
                    };
                    match handler.poll() {
                        Ok(Async::Ready(_)) => self.state = Some(Receiving),
                        Ok(Async::NotReady) => {
                            self.state = Some(Processing(handler));
                            return Ok(Async::NotReady);
                        }
                        Err(e) => return Err(e),
                    }
                }
                None => unreachable!(),
            };
        }
    }
}
