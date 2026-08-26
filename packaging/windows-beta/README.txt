Resonance Signal - Windows Beta
================================

1. Keep all files together in a stable folder.
2. Launch resonance-agent.exe.
3. Find Resonance Signal in the Windows notification area.
4. Open its tray menu and confirm Status: Running.
5. Optionally select Start with Windows.

Local health checks:
  http://127.0.0.1:48480/v1/status
  http://127.0.0.1:48480/v1/sources

Diagnostics log:
  %LOCALAPPDATA%\Resonance Signal\logs\resonance-signal.log

Command-line diagnostics remain available:
  resonance-agent.exe --help
  resonance-agent.exe capture --duration-seconds 10
  resonance-agent.exe serve

The service accepts local-machine loopback connections only. It is not
intended for LAN or Internet exposure. Compatible consumers are separate
applications.

Full setup and troubleshooting guidance is in the project README on GitHub.
