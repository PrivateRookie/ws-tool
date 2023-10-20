use std::borrow::Cow;

use crate::frame::OpCode;

/// generic message receive/send from websocket stream
#[derive(Debug)]
pub struct Message<T> {
    /// opcode of message
    ///
    /// see all codes in [overview](https://datatracker.ietf.org/doc/html/rfc6455#section-5.2) of opcode
    pub code: OpCode,
    /// payload of message
    pub data: T,

    /// available in close frame only
    ///
    /// see [status code](https://datatracker.ietf.org/doc/html/rfc6455#section-7.4)
    pub close_code: Option<u16>,
}

impl<T> Message<T> {
    /// consume message and return payload
    pub fn into(self) -> T {
        self.data
    }
}

impl<'a> From<&'a str> for Message<Cow<'a, str>> {
    fn from(data: &'a str) -> Self {
        Message {
            code: OpCode::Text,
            data: Cow::Borrowed(data),
            close_code: None,
        }
    }
}

impl<'a> From<&'a String> for Message<Cow<'a, str>> {
    fn from(data: &'a String) -> Self {
        Message {
            code: OpCode::Text,
            data: Cow::Borrowed(data),
            close_code: None,
        }
    }
}

impl<'a> From<Cow<'a, str>> for Message<Cow<'a, str>> {
    fn from(data: Cow<'a, str>) -> Self {
        Message {
            code: OpCode::Text,
            data,
            close_code: None,
        }
    }
}

impl<'a> From<&'a [u8]> for Message<Cow<'a, [u8]>> {
    fn from(data: &'a [u8]) -> Self {
        Message {
            code: OpCode::Binary,
            data: Cow::Borrowed(data),
            close_code: None,
        }
    }
}

impl<'a> From<Cow<'a, [u8]>> for Message<Cow<'a, [u8]>> {
    fn from(data: Cow<'a, [u8]>) -> Self {
        Message {
            code: OpCode::Binary,
            data,
            close_code: None,
        }
    }
}

impl<'a, T: Into<Cow<'a, str>>> From<(u16, T)> for Message<Cow<'a, str>> {
    fn from((close_code, value): (u16, T)) -> Self {
        Message {
            code: OpCode::Close,
            data: value.into(),
            close_code: Some(close_code),
        }
    }
}

impl<'a, T: Into<Cow<'a, [u8]>>> From<(u16, T)> for Message<Cow<'a, [u8]>> {
    fn from((close_code, value): (u16, T)) -> Self {
        Message {
            code: OpCode::Binary,
            data: value.into(),
            close_code: Some(close_code),
        }
    }
}

impl<'a, T: Into<Cow<'a, str>>> From<(OpCode, T)> for Message<Cow<'a, str>> {
    fn from((code, value): (OpCode, T)) -> Self {
        Message {
            code,
            data: value.into(),
            close_code: None,
        }
    }
}

impl<'a, T: Into<Cow<'a, [u8]>>> From<(OpCode, T)> for Message<Cow<'a, [u8]>> {
    fn from((code, value): (OpCode, T)) -> Self {
        Message {
            code,
            data: value.into(),
            close_code: None,
        }
    }
}
