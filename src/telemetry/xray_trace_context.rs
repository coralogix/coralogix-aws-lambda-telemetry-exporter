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

use hex::FromHexError;
use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

#[derive(Clone)]
pub struct XRayTraceContext {
    pub root: Vec<u8>,
    pub parent: Vec<u8>,
    pub sampled: bool,
}

impl TryFrom<&str> for XRayTraceContext {
    type Error = crate::Error;

    fn try_from(s: &str) -> Result<XRayTraceContext, lambda_extension::Error> {
        lazy_static! {
            // Example input: Root=1-6352a70e-1e2c502e358361800241fd45;Parent=35465b3a9e2f7c6a;Sampled=1
            static ref RE: Regex = Regex::new(r#"Root=1-([0-9a-f]{0,8})-([0-9a-f]{0,24});Parent=([0-9a-f]{0,16});Sampled=([0-1])?"#).unwrap(); // TODO Parent should be optional
        }

        match RE.captures(s) {
            Some(cap) => {
                let groups = (cap.get(1), cap.get(2), cap.get(3), cap.get(4));
                match groups {
                    (Some(root1), Some(root2), Some(parent), Some(sampled)) => {
                        let mut root1_bytes = decode_hex(root1.as_str(), 8)?;
                        let mut root2_bytes = decode_hex(root2.as_str(), 24)?;
                        root1_bytes.append(&mut root2_bytes);

                        Ok(XRayTraceContext {
                            root: root1_bytes,
                            parent: decode_hex(parent.as_str(), 16)?,
                            sampled: sampled.as_str() == "1",
                        })
                    }
                    _ => Err(Box::new(XRayTraceError::InvalidTraceContextFormat {
                        found: s.to_owned(),
                    })),
                }
            }
            None => Err(Box::new(XRayTraceError::InvalidTraceContextFormat {
                found: s.to_owned(),
            })),
        }
    }
}

fn decode_hex(s: &str, expected_characters: usize) -> Result<Vec<u8>, FromHexError> {
    if s.len() >= expected_characters {
        hex::decode(s)
    } else {
        // if the hex string is too short, prepend zeroes
        let mut prefix = String::with_capacity(expected_characters);
        for _ in 0..(expected_characters - s.len()) {
            prefix.push('0')
        }
        prefix.push_str(s);
        hex::decode(&prefix)
    }
}

#[derive(Error, Debug)]
pub enum XRayTraceError {
    #[error("Couldn't parse the XRay TraceContext {found:?})")]
    InvalidTraceContextFormat { found: String },
}

#[cfg(test)]
mod tests {
    use super::XRayTraceContext;

    #[test]
    fn parse_xray_trace_context() {
        let input = "Root=1-6352a70e-1e2c502e358361800241fd45;Parent=35465b3a9e2f7c6a;Sampled=1";
        let result = XRayTraceContext::try_from(input).unwrap();
        assert_eq!(
            result.root,
            vec!(
                0x63, 0x52, 0xa7, 0x0e, 0x1e, 0x2c, 0x50, 0x2e, 0x35, 0x83, 0x61, 0x80, 0x02, 0x41,
                0xfd, 0x45
            )
        );
        assert_eq!(
            result.parent,
            vec!(0x35, 0x46, 0x5b, 0x3a, 0x9e, 0x2f, 0x7c, 0x6a)
        );
        assert!(result.sampled);
    }

    #[test]
    fn ignore_surplus_elements() {
        let input = "Root=1-6352a70e-1e2c502e358361800241fd45;Parent=35465b3a9e2f7c6a;Sampled=1;Lineage=ae8024b8:0";
        let result = XRayTraceContext::try_from(input).unwrap();
        assert_eq!(
            result.root,
            vec!(
                0x63, 0x52, 0xa7, 0x0e, 0x1e, 0x2c, 0x50, 0x2e, 0x35, 0x83, 0x61, 0x80, 0x02, 0x41,
                0xfd, 0x45
            )
        );
        assert_eq!(
            result.parent,
            vec!(0x35, 0x46, 0x5b, 0x3a, 0x9e, 0x2f, 0x7c, 0x6a)
        );
        assert!(result.sampled);
    }

    // I think AWS doesn't print leading zeroes
    // https://eu-west-1.console.aws.amazon.com/cloudwatch/home?region=eu-west-1#logsV2:log-groups/log-group/$252Faws$252Flambda$252Flambda-test-JavaFailOtel-CcaKRulGoO8r/log-events/2023$252F08$252F21$252F$255B$2524LATEST$255D5fc5103003834dbd96abf818d654f4be
    #[test]
    fn accept_less_characters() {
        let input = "Root=1-a886f2-532b2c0dbc84cbe0b43f33b;Parent=97f845c0271e5c9;Sampled=1;Lineage=81c4d629:0";
        let result = XRayTraceContext::try_from(input).unwrap();
        assert_eq!(
            result.root,
            vec!(
                0x00, 0xa8, 0x86, 0xf2, 0x05, 0x32, 0xb2, 0xc0, 0xdb, 0xc8, 0x4c, 0xbe, 0x0b, 0x43,
                0xf3, 0x3b
            )
        );
        assert_eq!(
            result.parent,
            vec!(0x09, 0x7f, 0x84, 0x5c, 0x02, 0x71, 0xe5, 0xc9)
        );
        assert!(result.sampled);
    }
}
