use async_trait::async_trait;
use cbor4ii::core::error::DecodeError;
use futures::prelude::*;
use libp2p::StreamProtocol;
use libp2p::request_response;
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::TryReserveError, convert::Infallible, io, marker::PhantomData};

const REQUEST_SIZE_MAXIMUM: u64 = 1024 * 1024;
const RESPONSE_SIZE_MAXIMUM: u64 = 64 * 1024 * 1024;

pub type Behaviour<Req, Resp> = request_response::Behaviour<CborCodec<Req, Resp>>;

#[derive(Clone)]
pub struct CborCodec<Req, Resp> {
    phantom: PhantomData<(Req, Resp)>,
}

impl<Req, Resp> Default for CborCodec<Req, Resp> {
    fn default() -> Self {
        Self {
            phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<Req, Resp> request_response::Codec for CborCodec<Req, Resp>
where
    Req: Send + Serialize + DeserializeOwned,
    Resp: Send + Serialize + DeserializeOwned,
{
    type Protocol = StreamProtocol;
    type Request = Req;
    type Response = Resp;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Req>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();

        io.take(REQUEST_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

        cbor4ii::serde::from_slice(vec.as_slice()).map_err(decode_into_io_error)
    }

    async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Resp>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();

        io.take(RESPONSE_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

        cbor4ii::serde::from_slice(vec.as_slice()).map_err(decode_into_io_error)
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data: Vec<u8> =
            cbor4ii::serde::to_vec(Vec::new(), &req).map_err(encode_into_io_error)?;

        io.write_all(data.as_ref()).await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        resp: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data: Vec<u8> =
            cbor4ii::serde::to_vec(Vec::new(), &resp).map_err(encode_into_io_error)?;

        io.write_all(data.as_ref()).await?;

        Ok(())
    }
}

fn decode_into_io_error(err: cbor4ii::serde::DecodeError<Infallible>) -> io::Error {
    match err {
        cbor4ii::serde::DecodeError::Core(DecodeError::Read(e)) => {
            io::Error::new(io::ErrorKind::Other, e)
        }
        cbor4ii::serde::DecodeError::Core(e @ DecodeError::Unsupported { .. }) => {
            io::Error::new(io::ErrorKind::Unsupported, e)
        }
        cbor4ii::serde::DecodeError::Core(e @ DecodeError::Eof { .. }) => {
            io::Error::new(io::ErrorKind::UnexpectedEof, e)
        }
        cbor4ii::serde::DecodeError::Core(e) => io::Error::new(io::ErrorKind::InvalidData, e),
        cbor4ii::serde::DecodeError::Custom(e) => {
            io::Error::new(io::ErrorKind::Other, e.to_string())
        }
    }
}

fn encode_into_io_error(err: cbor4ii::serde::EncodeError<TryReserveError>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err)
}
