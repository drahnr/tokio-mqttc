use std::collections::hash_map::Entry;
use std::result;

use tokio_io::{AsyncRead, AsyncWrite};
use futures::{Poll, Async, Stream, Future};
use futures::sync::mpsc::unbounded;
use futures::unsync::mpsc::UnboundedSender;
use futures_mutex::{FutMutex, FutMutexGuard};

use persistence::Persistence;
use proto::{MqttPacket, QualityOfService, ConnectReturnCode, ConnectAckFlags};
use errors::{Error, ErrorKind};
use errors::proto::ErrorKind as ProtoErrorKind;
use types::SubItem;
use super::mqtt_loop::LoopData;
use super::inbound_handlers::ResponseHandler;
use super::{SourceError, ClientReturn, OneTimeKey, MqttFramedReader, LoopRequest};

enum State<'p> {
    Receiving,
    Processing(ResponseHandler<'p>),
}

/// This type will take a packet from the server and process it, returning the stream it came from
pub struct ResponseProcessor<'p, I, P>
where
    I: AsyncRead + AsyncWrite + Send,
    P: 'p + Send + Persistence,
{
    state: Option<State<'p>>,
    stream: MqttFramedReader<I>,
    req_chan: UnboundedSender<LoopRequest>,
    data_lock: FutMutex<LoopData<'p, P>>,
}

impl<'p, I, P> ResponseProcessor<'p, I, P>
where
    I: AsyncRead + AsyncWrite + Send,
    P: 'p + Send + Persistence,
{
    pub fn new(
        packet: MqttPacket,
        stream: MqttFramedReader<I>,
        req_chan: UnboundedSender<LoopRequest>,
        data_lock: FutMutex<LoopData<'p, P>>,
    ) -> ResponseProcessor<'p, I, P> {

        ResponseProcessor {
            state: Some(State::Receiving),
            stream,
            req_chan,
            data_lock,
        }
    }
}

impl<'p, I, P> Future for ResponseProcessor<'p, I, P>
where
    I: AsyncRead + AsyncWrite + Send,
    P: 'p + Send + Persistence,
{
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use self::State::*;
        println!("Poll ResponseProcessor");
        loop {
            match self.state {
                Some(Receiving) => {
                    let read = match try_ready!(self.stream.poll()) {
                        Some(res) => res,
                        None => return Err(ErrorKind::UnexpectedDisconnect.into()),
                    };

                    read.validate()?;

                    self.state = Some(State::Processing(ResponseHandler::new(
                        read,
                        self.req_chan.clone(),
                        self.data_lock.clone(),
                    )))
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
