
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

struct Connect {
}

impl Connect {
    pub fn new() -> Self {
        Self {}
    }
}


impl Future for Connect {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(Async::Ready(()))
    }
}
