// Copyright 2026 Coralogix Ltd.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//         http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{FromRequest, Request};
use axum::response::IntoResponse;
use hyper::{StatusCode, header};
use mime::Mime;
use tonic::codec::{Codec, Decoder};
use tonic_prost::ProstCodec;

/// Message is a simple wrapper around a protobuf message
/// that preserves the size of the original binary representation.
pub struct ProtoBuf<T> {
    pub message: T,
}

#[allow(dead_code)]
#[derive(Default)]
pub struct ProtoBufCodec<T, U> {
    codec: ProstCodec<T, U>,
}

#[allow(dead_code)]
pub struct ProtoBufDecoder<Item, C: Decoder<Item = Item>>(C);

#[derive(Debug)]
pub enum ProtoBufRejection {
    ContentTypeUnsupported(Option<Mime>),
    Bytes(BytesRejection),
    DecoderFailed(prost::DecodeError),
}

impl<T> From<T> for ProtoBuf<T>
where
    T: prost::Message,
{
    fn from(message: T) -> Self {
        ProtoBuf { message }
    }
}

impl<T> IntoResponse for ProtoBuf<T>
where
    T: prost::Message,
{
    fn into_response(self) -> axum::response::Response {
        self.message.encode_to_vec().into_response()
    }
}

impl IntoResponse for ProtoBufRejection {
    fn into_response(self) -> axum::response::Response {
        match self {
            ProtoBufRejection::ContentTypeUnsupported(mime) => (
                StatusCode::BAD_REQUEST,
                format!("Unsupported Content-Type: {mime:?}"),
            )
                .into_response(),
            ProtoBufRejection::Bytes(b) => b.into_response(),
            ProtoBufRejection::DecoderFailed(error) => (
                StatusCode::BAD_REQUEST,
                format!("Failed to parse the request body as Protobuf: {error:?}"),
            )
                .into_response(),
        }
    }
}

impl<S, T> FromRequest<S> for ProtoBuf<T>
where
    T: prost::Message + Default,
    S: Send + Sync,
{
    type Rejection = ProtoBufRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let mime = content_type_mime(&req)?;
        match (mime.type_(), mime.subtype().as_str()) {
            (mime::APPLICATION, "x-protobuf") => {
                let bytes = Bytes::from_request(req, state)
                    .await
                    .map_err(ProtoBufRejection::Bytes)?;
                let message: T =
                    prost::Message::decode(bytes).map_err(ProtoBufRejection::DecoderFailed)?;
                Ok(ProtoBuf { message })
            }
            _ => Err(ProtoBufRejection::ContentTypeUnsupported(Some(mime))),
        }
    }
}

impl<T, U> Codec for ProtoBufCodec<T, U>
where
    T: prost::Message + Send + 'static,
    U: prost::Message + Default + Send + 'static,
{
    type Encode = T;
    type Decode = ProtoBuf<U>;
    type Encoder = <ProstCodec<T, U> as tonic::codec::Codec>::Encoder;
    type Decoder = ProtoBufDecoder<U, <ProstCodec<T, U> as tonic::codec::Codec>::Decoder>;

    fn encoder(&mut self) -> Self::Encoder {
        self.codec.encoder()
    }

    fn decoder(&mut self) -> Self::Decoder {
        ProtoBufDecoder(self.codec.decoder())
    }
}

impl<Item: prost::Message, C: Decoder<Item = Item>> Decoder for ProtoBufDecoder<Item, C> {
    type Item = ProtoBuf<Item>;
    type Error = C::Error;

    fn decode(
        &mut self,
        src: &mut tonic::codec::DecodeBuf<'_>,
    ) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(message) = self.0.decode(src)? {
            Ok(Some(ProtoBuf { message }))
        } else {
            Ok(None)
        }
    }
}

fn content_type_mime<B>(req: &Request<B>) -> Result<Mime, ProtoBufRejection> {
    let content_type = if let Some(content_type) = req.headers().get(header::CONTENT_TYPE) {
        content_type
    } else {
        return Err(ProtoBufRejection::ContentTypeUnsupported(None));
    };

    let content_type = if let Ok(content_type) = content_type.to_str() {
        content_type
    } else {
        return Err(ProtoBufRejection::ContentTypeUnsupported(None));
    };

    let mime = if let Ok(mime) = content_type.parse::<Mime>() {
        mime
    } else {
        return Err(ProtoBufRejection::ContentTypeUnsupported(None));
    };

    Ok(mime)
}
