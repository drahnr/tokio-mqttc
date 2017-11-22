#[macro_use]
extern crate futures;
extern crate tokio_mqttc;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_tls;
#[macro_use]
extern crate error_chain;

use tokio_mqttc::client::{Client, ClientConfig};
use tokio_mqttc::persistence::*;
use tokio_mqttc::proto::*;
use tokio_mqttc::proto::*;

mod errors {
    use std;
    error_chain! {
        types {
            Error, ErrorKind, ResultExt, Result;
        }

        links {
            TokioMqttc(::tokio_mqttc::errors::Error, ::tokio_mqttc::errors::ErrorKind);
        }

        foreign_links {
            Io(std::io::Error) #[cfg(unix)];
            AddrParse(std::net::AddrParseError);
        }

        errors {
            DummyError {
                description("A placeholder")
            }
        }
    }
}

use errors::*;

use tokio_core::reactor::Handle;
use tokio_core::reactor::Core;


use futures::{future, Future, Stream, Sink};

fn run() -> Result<()> {
    let name = "funbunny".to_owned();
    let cfg = ClientConfig::new()
        .connect_timeout(30)
        .keep_alive(15)
        .client_id(name);

    let mut core = Core::new().chain_err(|| ErrorKind::DummyError)?;


    println!("socket addr parse");

    let sock_addr: std::net::SocketAddr = "127.0.0.1:1883".parse()?;

    let mut client = Client::new(
        tokio_mqttc::persistence::MemoryPersistence::new(),
        core.handle(),
    )?;

    let work = tokio_core::net::TcpStream::connect(&sock_addr, &core.handle())
        .map_err(|_| tokio_mqttc::errors::ErrorKind::ClientUnavailable.into())
        .and_then(|io| {
            println!("tcp connect");



            client.connect(io, &cfg).and_then(|(client, lp, _opt_old)| {
//                 use futures::stream::FuturesUnordered;

//                 let mut fm = Box::new(FuturesUnordered::new());
// fm.into_future()
                println!("pub publish");

                client.publish(
                    "xx".to_owned(),
                    QualityOfService::QoS0,
                    false,
                    "abcdef".as_bytes().into(),
                );

                Ok(lp)
            })
            //.map_err(|_| ErrorKind::DummyError.into())
        });
    // let x : u8 = work;

    core.run(work).unwrap();
    Ok(())
}

quick_main!(run);
