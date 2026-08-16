-- The interpreter this account wants the app to start.
--
-- 'auto' keeps the host probe order (PowerShell before cmd.exe on Windows,
-- bash before sh elsewhere); 'powershell', 'bash' and 'cmd' pin the choice, with
-- 'bash' meaning Git for Windows' bash.exe on a Windows host. A pinned shell
-- that is not installed falls back to the probe order rather than failing the
-- call, so an uninstalled preference costs the agent its dialect, not its
-- ability to run a command.
ALTER TABLE system_settings
ADD COLUMN shell_preference TEXT NOT NULL DEFAULT 'auto';
