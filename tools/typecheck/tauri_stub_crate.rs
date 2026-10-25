//! tauri 的最小替身，编译成独立 crate 供命令层检查使用。
//!
//! 只覆盖命令层实际用到的符号：AppHandle、Manager::path()、
//! path::BaseDirectory 与 PathResolver::resolve、以及 async_runtime::spawn_blocking。
//! #[tauri::command] 是过程宏，无法用源码替代，
//! 检查时由 gen_cmd_check.py 把该属性剥掉。

use std::path::PathBuf;

#[derive(Clone)]
pub struct AppHandle {
    _priv: (),
}

impl AppHandle {
    pub fn new_for_test() -> Self {
        Self { _priv: () }
    }
}

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 异步运行时里命令层用到的那部分。
///
/// 形状必须与真实 tauri 一致：`spawn_blocking` 返回的句柄 `.await` 之后得到
/// `Result<T, Error>`，命令层的 `spawn_blocking(..).await.map_err(..)` 才检查得下去。
///
/// 桩**真的把任务跑掉**并立刻给出结果，而不是永远挂着：命令层的单元测试
/// 会经 `block_on` 跑整条链路（见 commands/duplicates.rs 里那条异步外壳用例），
/// 若此处返回 Pending，那个测试会一直转下去（run.ps1 没有超时）。
pub mod async_runtime {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    pub struct JoinHandle<T>(Option<Result<T, super::Error>>);

    /// 只对 `T: Unpin` 实现 Future。
    ///
    /// 需要这个约束才能从 `Pin<&mut Self>` 里取出内部可变引用（`get_mut`）；
    /// 命令层 await 的结果类型都是 Unpin 的普通结构体，因此不受影响。
    impl<T: Unpin> Future for JoinHandle<T> {
        type Output = Result<T, super::Error>;

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            match self.get_mut().0.take() {
                Some(v) => Poll::Ready(v),
                // 只应被 poll 一次：真实实现里重复 poll 已完成的句柄是错的
                None => panic!("JoinHandle 只能被 await 一次"),
            }
        }
    }

    pub fn spawn_blocking<F, R>(task: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        JoinHandle(Some(Ok(task())))
    }

    /// 把 future 跑到完成。真 tauri 走 tokio runtime，这里循环 poll 即可——
    /// 桩里的任务总是立刻完成，不需要真正的唤醒机制。
    pub fn block_on<F: Future>(task: F) -> F::Output {
        let mut task = Box::pin(task);
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        loop {
            if let Poll::Ready(v) = task.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    /// 用 std::task::Wake 造一个空唤醒器，避免手写 RawWaker 与 unsafe。
    fn noop_waker() -> Waker {
        struct Noop;
        impl std::task::Wake for Noop {
            fn wake(self: std::sync::Arc<Self>) {}
        }
        Waker::from(std::sync::Arc::new(Noop))
    }
}

pub mod path {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum BaseDirectory {
        Audio, Cache, Config, Data, LocalData, Document, Download, Picture,
        Public, Video, Resource, Temp, AppConfig, AppData, AppLocalData,
        AppCache, AppLog, Desktop, Executable, Font, Home, Runtime, Template,
    }

    pub struct PathResolver {
        _priv: (),
    }

    impl PathResolver {
        /// 仅桩内部使用
        pub(crate) fn new() -> Self {
            Self { _priv: () }
        }

        pub fn resolve<P: AsRef<std::path::Path>>(
            &self,
            path: P,
            _dir: BaseDirectory,
        ) -> Result<std::path::PathBuf, super::Error> {
            Ok(path.as_ref().to_path_buf())
        }

        /// 应用配置目录。界面状态存在这里（见 commands::ui_state_path）。
        ///
        /// 真实 tauri 由平台约定决定具体路径；离线检查只验证类型与调用形态，
        /// 因此这里给一个合法且确定的位置即可。
        pub fn app_config_dir(&self) -> Result<std::path::PathBuf, super::Error> {
            Ok(std::path::PathBuf::from("config"))
        }
    }
}

pub trait Manager {
    fn path(&self) -> path::PathResolver;
}

impl Manager for AppHandle {
    fn path(&self) -> path::PathResolver {
        path::PathResolver::new()
    }
}

pub fn stub_path(p: &str) -> PathBuf {
    PathBuf::from(p)
}

/// 事件发送。真实 tauri 要求 payload: Serialize + Clone；这里放宽为任意类型，
/// 只验证命令层调用形状。
pub trait Emitter {
    fn emit<T>(&self, _event: &str, _payload: T) -> Result<(), Error>;
}

impl Emitter for AppHandle {
    fn emit<T>(&self, _event: &str, _payload: T) -> Result<(), Error> {
        Ok(())
    }
}

impl path::PathResolver {
    /// 应用数据目录。离线检查只验证调用形状。
    pub fn app_data_dir(&self) -> Result<std::path::PathBuf, Error> {
        Ok(std::path::PathBuf::from("data"))
    }
}
