use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, error::Error, fmt};

/// A Fluent argument with an explicit type (integers support plural selection).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageArg {
    String(String),
    Integer(i64),
    Message(Box<LocalizedMessage>),
}

impl From<String> for MessageArg {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<&str> for MessageArg {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}
impl From<i64> for MessageArg {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
impl From<i32> for MessageArg {
    fn from(value: i32) -> Self {
        Self::Integer(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizedMessage {
    pub id: String,
    pub args: BTreeMap<String, MessageArg>,
}

impl LocalizedMessage {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            args: BTreeMap::new(),
        }
    }

    pub fn with_arg(mut self, name: impl Into<String>, value: impl Into<MessageArg>) -> Self {
        self.args.insert(name.into(), value.into());
        self
    }
}

/// Raw text is deliberate: paths, external output, and user data are never IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalizedText {
    Message(LocalizedMessage),
    Raw(String),
}

impl From<LocalizedMessage> for LocalizedText {
    fn from(message: LocalizedMessage) -> Self {
        Self::Message(message)
    }
}

/// Stable event identity and a deferred user-facing message with a typed cause chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizedError {
    pub event_code: String,
    pub message: LocalizedMessage,
    pub cause: Option<Box<LocalizedError>>,
}

impl LocalizedError {
    pub fn new(event_code: impl Into<String>, message: LocalizedMessage) -> Self {
        Self {
            event_code: event_code.into(),
            message,
            cause: None,
        }
    }

    pub fn with_cause(mut self, cause: LocalizedError) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

impl fmt::Display for LocalizedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.event_code, self.message.id)
    }
}

impl Error for LocalizedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_deref().map(|cause| cause as &dyn Error)
    }
}

impl LocalizedMessage {
    pub fn render(&self, snapshot: &crate::LanguageSnapshot) -> String {
        snapshot.render(self)
    }
    pub fn render_current(&self) -> String {
        crate::render_current(self)
    }
}
impl LocalizedText {
    pub fn render_current(&self) -> String {
        match self {
            Self::Message(message) => crate::render_current(message),
            Self::Raw(raw) => raw.clone(),
        }
    }
}
impl From<String> for LocalizedText {
    fn from(value: String) -> Self {
        Self::Raw(value)
    }
}
impl From<&str> for LocalizedText {
    fn from(value: &str) -> Self {
        Self::Raw(value.to_owned())
    }
}
impl From<&String> for MessageArg {
    fn from(value: &String) -> Self {
        Self::String(value.clone())
    }
}
macro_rules! integer_args {
    ($($ty:ty),*) => {$ (
        impl From<$ty> for MessageArg {
            fn from(value: $ty) -> Self {
                // Preserve very large unsigned integers as decimal text rather than wrapping.
                i64::try_from(value).map(Self::Integer).unwrap_or_else(|_| Self::String(value.to_string()))
            }
        }
    )*};
}
integer_args!(i8, i16, isize, u8, u16, u32, u64, usize, i128, u128);

impl From<LocalizedMessage> for MessageArg {
    fn from(value: LocalizedMessage) -> Self {
        Self::Message(Box::new(value))
    }
}
impl From<LocalizedText> for MessageArg {
    fn from(value: LocalizedText) -> Self {
        match value {
            LocalizedText::Raw(value) => Self::String(value),
            LocalizedText::Message(value) => value.into(),
        }
    }
}
impl From<&LocalizedMessage> for MessageArg {
    fn from(value: &LocalizedMessage) -> Self {
        value.clone().into()
    }
}
impl From<&LocalizedText> for MessageArg {
    fn from(value: &LocalizedText) -> Self {
        value.clone().into()
    }
}
