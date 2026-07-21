#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Role {
    System = 0,
    User = 1,
    Assistant = 2,
    Tool = 3,
}

impl Role {
    pub const fn val(&self) -> u8 {
        *self as u8
    }

    pub const fn from_val(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::System),
            1 => Some(Self::User),
            2 => Some(Self::Assistant),
            3 => Some(Self::Tool),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: Vec<u8>,
}