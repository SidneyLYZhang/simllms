#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ContentBlock {
    Text(String),
    Image(ImageSource),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImageSource {
    Base64 { data: Vec<u8>, mime_type: String },
    Url(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn text(content: impl Into<String>) -> Self {
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text(content.into())],
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Message {
            role: Role::System,
            content: vec![ContentBlock::Text(content.into())],
        }
    }

    pub fn blocks(role: Role, content: Vec<ContentBlock>) -> Self {
        Message { role, content }
    }
}

impl From<&str> for Message {
    fn from(s: &str) -> Self {
        Message::text(s)
    }
}

impl From<String> for Message {
    fn from(s: String) -> Self {
        Message::text(s)
    }
}
