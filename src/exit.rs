use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success,
    InvalidArgs,
    ScanFailed,
    ExternalCommandFailed,
}

impl ExitCode {
    pub const fn as_i32(self) -> i32 {
        match self {
            ExitCode::Success => 0,
            ExitCode::InvalidArgs => 2,
            ExitCode::ScanFailed => 10,
            ExitCode::ExternalCommandFailed => 20,
        }
    }
}

#[derive(Debug)]
pub struct ExitError {
    pub code: ExitCode,
    pub err: anyhow::Error,
}

impl ExitError {
    pub fn new(code: ExitCode, err: anyhow::Error) -> Self {
        Self { code, err }
    }
}

impl fmt::Display for ExitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.err.fmt(f)
    }
}

impl std::error::Error for ExitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.err.as_ref())
    }
}

pub fn exit_code(err: &anyhow::Error) -> i32 {
    if let Some(exit) = err.downcast_ref::<ExitError>() {
        return exit.code.as_i32();
    }
    ExitCode::ScanFailed.as_i32()
}

pub fn invalid_args(message: impl Into<String>) -> anyhow::Error {
    ExitError::new(ExitCode::InvalidArgs, anyhow::anyhow!(message.into())).into()
}

pub fn invalid_args_err(err: anyhow::Error) -> anyhow::Error {
    ExitError::new(ExitCode::InvalidArgs, err).into()
}

pub fn external_cmd(message: impl Into<String>) -> anyhow::Error {
    ExitError::new(
        ExitCode::ExternalCommandFailed,
        anyhow::anyhow!(message.into()),
    )
    .into()
}

pub fn external_cmd_err(err: anyhow::Error) -> anyhow::Error {
    ExitError::new(ExitCode::ExternalCommandFailed, err).into()
}
