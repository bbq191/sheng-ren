//! 书库内部的错误：种类用来分派，`Display` 照旧输出给人看的那一行（原文逐字不变）。
//!
//! 以前 `sync` 靠错误字符串开头认「设备满了」「设备掉线」「文件正在写入」（`NO_SPACE`、`UNMOUNTED`、`BUSY`），
//! 改文案就会悄悄改掉控制流。现在由构造错误的那一处带上 [`Kind`]，分派只看种类；字符串常量只用来拼给用户看的文字。

use std::fmt;

/// 错误种类：调用方按它决定停不停这台设备、记不记失败表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// 设备写满了（ENOSPC）：这一轮不再往这台写，不记失败表。
    NoSpace,
    /// 设备中途断开、挂载点不在了：记成掉线，下一轮接上重传。
    Unmounted,
    /// 原件还在被写（读的过程中大小或修改时间变了）：这次不算数，下一轮再试。
    Busy,
    /// 其它错误：照常报失败。
    Other,
}

/// 带种类的错误：给人看的文字在 `message` 里，分派看 `kind`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: Kind,
    message: String,
}

impl Error {
    pub(crate) fn new(kind: Kind, message: impl Into<String>) -> Self {
        Error {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn other(message: impl Into<String>) -> Self {
        Error::new(Kind::Other, message)
    }

    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Error::other(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Error::other(message)
    }
}
