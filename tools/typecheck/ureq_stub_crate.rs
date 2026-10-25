//! ureq 的最小替身，编译成独立 crate 供离线类型检查使用。
//!
//! 只提供被检查代码实际用到的签名：AgentBuilder 的超时配置、Agent::get、
//! Request::set / call、Response 的状态码 / 头部 / 正文读取，以及错误类型。
//! 语义刻意做成「请求失败」：需要联网的测试都标了 #[ignore]，本桩只保证
//! 调用形状可编译，不伪造网络数据。

use std::io::Read;
use std::time::Duration;

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

#[derive(Default)]
pub struct Agent {
    _priv: (),
}

pub struct AgentBuilder {
    agent: Agent,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self { agent: Agent::default() }
    }

    pub fn timeout(self, _timeout: Duration) -> Self {
        self
    }

    pub fn timeout_connect(self, _timeout: Duration) -> Self {
        self
    }

    pub fn timeout_read(self, _timeout: Duration) -> Self {
        self
    }

    pub fn build(self) -> Agent {
        self.agent
    }
}

impl Agent {
    pub fn get(&self, url: &str) -> Request {
        Request { url: url.to_string() }
    }
}

pub struct Request {
    url: String,
}

impl Request {
    pub fn set(self, _name: &str, _value: &str) -> Self {
        self
    }

    pub fn call(self) -> Result<Response, Error> {
        Err(Error(format!("离线桩不发起请求：{}", self.url)))
    }
}

pub struct Response {
    body: Vec<u8>,
}

impl Response {
    pub fn status(&self) -> u16 {
        200
    }

    pub fn header(&self, _name: &str) -> Option<&str> {
        None
    }

    pub fn into_reader(self) -> Box<dyn Read> {
        Box::new(std::io::Cursor::new(self.body))
    }

    pub fn into_string(self) -> Result<String, std::io::Error> {
        String::from_utf8(self.body).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
