use std::process::Command as StdCommand;

use tokio::process::Command as TokioCommand;

/// `CreateNoWindow` process-creation flag, so a Windows GUI session does not
/// flash a console window when spawning a CLI child process.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Convert an app-managed std command into a Tokio command, suppressing the
/// Windows console window that CLI children would otherwise create.
pub(crate) fn tokio_command_no_window(mut command: StdCommand) -> TokioCommand {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    TokioCommand::from(command)
}
