use std::io::{self, Read, Write};
use std::str;

use libflate::gzip::{Decoder, Encoder};
use prost::{DecodeError, Message};
use thiserror::Error;

/// Compressor represents compression algorithm
pub(crate) enum Compressor {
    /// Plain does not apply any compression algorithm to the encoded data.
    Plain,
    /// Gzip compresses the data with the gzip algorithm after encoding it.
    Gzip,
}

#[derive(Error, Debug, PartialEq)]
pub enum CompressorError {
    #[error("encoding format not supported: `{0}`")]
    UnsupportedEncoding(String),
}

// TryFrom returns a compressor type given an slice of bytes
// Only gzip format is supported
impl TryFrom<&[u8]> for Compressor {
    type Error = CompressorError;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        match value {
            b"gzip" => Ok(Compressor::Gzip),
            val => match str::from_utf8(val) {
                Ok(s) => Err(CompressorError::UnsupportedEncoding(s.to_string())),
                Err(_) => Err(CompressorError::UnsupportedEncoding(format!("{:?}", val))),
            },
        }
    }
}

#[derive(Error, Debug)]
pub enum EncoderError {
    #[error("`{0}`")]
    IO(#[from] io::Error),
}

#[derive(Error, Debug)]
pub enum DecoderError {
    #[error("`{0}`")]
    Prost(#[from] DecodeError),
    #[error("`{0}`")]
    IO(#[from] io::Error),
}

/// encode_message encodes the provided message as a Protobuffer and compresses the result
/// with the provided algorithm
pub(crate) fn encode_message<M>(comp: &Compressor, msg: M) -> Result<Vec<u8>, EncoderError>
where
    M: Message,
{
    let proto_enc = msg.encode_to_vec();
    match comp {
        Compressor::Plain => Ok(proto_enc),
        Compressor::Gzip => {
            let mut encoder = Encoder::new(Vec::new())?;
            encoder.write_all(&proto_enc)?;
            Ok(encoder.finish().into_result()?)
        }
    }
}

/// decode_message extracts and decodes the Protobuffer message with the provided algorithm
pub(crate) fn decode_message<M>(comp: &Compressor, msg: &[u8]) -> Result<M, DecoderError>
where
    M: Message + Default,
{
    match comp {
        Compressor::Plain => Ok(M::decode(msg)?),
        Compressor::Gzip => {
            let mut decoder = Decoder::new(msg)?;
            let mut buf = Vec::new();
            decoder.read_to_end(&mut buf).unwrap();
            Ok(M::decode(buf.as_slice())?)
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use rand::distributions::{Alphanumeric, DistString};

    use crate::opamp::proto::{AgentConfigFile, AgentConfigMap, AgentToServer, EffectiveConfig};

    use super::*;

    #[test]
    fn empty_message() {
        let default_message = AgentToServer::default();

        let gzip_data = encode_message(&Compressor::Gzip, default_message.clone()).unwrap();
        let plain_data = encode_message(&Compressor::Plain, default_message.clone()).unwrap();

        // empty message with gzip contains gzip header
        assert!(gzip_data.len() > plain_data.len());

        assert_eq!(
            decode_message::<AgentToServer>(&Compressor::Plain, &plain_data).unwrap(),
            default_message
        );

        assert_eq!(
            decode_message::<AgentToServer>(&Compressor::Gzip, &gzip_data).unwrap(),
            default_message
        );
    }

    #[test]
    fn message_payload() {
        let effective_config = Some(EffectiveConfig {
            config_map: Some(AgentConfigMap {
                config_map: HashMap::from([(
                    "/test".to_string(),
                    AgentConfigFile {
                        body: Alphanumeric
                            .sample_string(&mut rand::thread_rng(), 300)
                            .as_bytes()
                            .to_vec(),
                        content_type: "random".to_string(),
                    },
                )]),
            }),
        });
        let sample_message = AgentToServer {
            effective_config,
            ..AgentToServer::default()
        };

        let gzip_data = encode_message(&Compressor::Gzip, sample_message.clone()).unwrap();
        let plain_data = encode_message(&Compressor::Plain, sample_message.clone()).unwrap();

        // big message should have smaller size with gzip compression
        assert!(gzip_data.len() < plain_data.len());

        assert_eq!(
            decode_message::<AgentToServer>(&Compressor::Plain, &plain_data).unwrap(),
            sample_message
        );

        assert_eq!(
            decode_message::<AgentToServer>(&Compressor::Gzip, &gzip_data).unwrap(),
            sample_message
        );
    }
}
