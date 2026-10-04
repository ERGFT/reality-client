// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.Diagnostics;
using System.IO;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

namespace RealityClientGui
{
    internal static class CoreRuntime
    {
        private const uint CtrlCEvent = 0;
        private const uint CtrlBreakEvent = 1;
        private const int SwHide = 0;
        private static ConsoleCtrlHandler handler = HandleConsoleControl;

        [UnmanagedFunctionPointer(CallingConvention.Winapi)]
        private delegate bool ConsoleCtrlHandler(uint controlType);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool AllocConsole();
        [DllImport("kernel32.dll")]
        private static extern IntPtr GetConsoleWindow();
        [DllImport("user32.dll")]
        private static extern bool ShowWindow(IntPtr window, int command);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool SetConsoleCtrlHandler(ConsoleCtrlHandler callback, bool add);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool GenerateConsoleCtrlEvent(uint controlEvent, uint processGroupId);

        private static bool HandleConsoleControl(uint controlType)
        {
            return controlType == 0 || controlType == CtrlBreakEvent;
        }

        public static void PrepareHiddenConsole()
        {
            AllocConsole();
            if (!SetConsoleCtrlHandler(handler, true))
                throw new InvalidOperationException("Не удалось подготовить безопасную остановку ядра.");
            IntPtr window = GetConsoleWindow();
            if (window != IntPtr.Zero) ShowWindow(window, SwHide);
        }

        private static string Quote(string value)
        {
            return "\"" + value.Replace("\"", "\\\"") + "\"";
        }

        private static ProcessStartInfo CreateInfo(string arguments)
        {
            return new ProcessStartInfo
            {
                FileName = ClientData.CorePath,
                Arguments = arguments,
                UseShellExecute = false,
                CreateNoWindow = false,
                WindowStyle = ProcessWindowStyle.Hidden,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                WorkingDirectory = ClientData.DirectoryPath
            };
        }

        public static Process Start(string configPath, Action<string> safeLog, bool systemProxy)
        {
            ProcessStartInfo info = CreateInfo("--config " + Quote(configPath) + (systemProxy ? " --system-proxy" : ""));
            Process process = new Process { StartInfo = info, EnableRaisingEvents = true };
            process.OutputDataReceived += delegate(object sender, DataReceivedEventArgs e)
            {
                if (!String.IsNullOrWhiteSpace(e.Data) && safeLog != null) safeLog(e.Data);
            };
            process.ErrorDataReceived += delegate(object sender, DataReceivedEventArgs e)
            {
                if (!String.IsNullOrWhiteSpace(e.Data) && safeLog != null) safeLog(e.Data);
            };
            if (!process.Start()) throw new InvalidOperationException("Ядро не запустилось.");
            process.BeginOutputReadLine();
            process.BeginErrorReadLine();
            return process;
        }

        public static bool WaitForProcessReady(Process process, int timeoutMs)
        {
            Stopwatch timer = Stopwatch.StartNew();
            while (timer.ElapsedMilliseconds < timeoutMs)
            {
                if (process.HasExited) return false;
                Thread.Sleep(150);
            }
            return !process.HasExited;
        }

        public static string CheckConfiguration(string configPath, int timeoutMs)
        {
            return RunAndCapture("--config " + Quote(configPath) + " --check", timeoutMs);
        }

        public static string DisableSystemProxy(int timeoutMs)
        {
            return RunAndCapture("--system-proxy-off", timeoutMs);
        }

        private static string RunAndCapture(string arguments, int timeoutMs)
        {
            using (Process process = new Process())
            {
                process.StartInfo = CreateInfo(arguments);
                process.StartInfo.CreateNoWindow = true;
                StringBuilder output = new StringBuilder();
                process.OutputDataReceived += delegate(object sender, DataReceivedEventArgs e)
                {
                    if (e.Data != null && output.Length < 24000) output.AppendLine(e.Data);
                };
                process.ErrorDataReceived += delegate(object sender, DataReceivedEventArgs e)
                {
                    if (e.Data != null && output.Length < 24000) output.AppendLine(e.Data);
                };
                if (!process.Start()) throw new InvalidOperationException("Не удалось запустить проверку ядра.");
                process.BeginOutputReadLine(); process.BeginErrorReadLine();
                if (!process.WaitForExit(timeoutMs))
                {
                    try { process.Kill(); } catch { }
                    throw new TimeoutException("Проверка ядра превысила время ожидания.");
                }
                process.WaitForExit();
                if (process.ExitCode != 0)
                    throw new InvalidOperationException("Проверка ядра не пройдена.\r\n" + SafeText(output.ToString()));
                return SafeText(output.ToString());
            }
        }

        private static string SafeText(string value)
        {
            string[] lines = value.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries);
            StringBuilder safe = new StringBuilder();
            foreach (string line in lines)
            {
                string candidate = line;
                int scheme = candidate.IndexOf("vless://", StringComparison.OrdinalIgnoreCase);
                if (scheme >= 0) candidate = candidate.Substring(0, scheme) + "[секрет скрыт]";
                if (candidate.Length > 300) candidate = candidate.Substring(0, 300) + "…";
                safe.AppendLine(candidate);
            }
            return safe.ToString();
        }

        public static bool WaitForListener(Process process, int timeoutMs)
        {
            Stopwatch timer = Stopwatch.StartNew();
            while (timer.ElapsedMilliseconds < timeoutMs)
            {
                if (process.HasExited) return false;
                try
                {
                    using (TcpClient socket = new TcpClient())
                    {
                        IAsyncResult pending = socket.BeginConnect("127.0.0.1", 1080, null, null);
                        using (System.Threading.WaitHandle wait = pending.AsyncWaitHandle)
                        {
                            if (wait.WaitOne(200))
                            {
                                socket.EndConnect(pending);
                                return true;
                            }
                        }
                    }
                }
                catch { }
                Thread.Sleep(150);
            }
            return false;
        }

        public static bool StopGracefully(Process process, int timeoutMs)
        {
            if (process == null) return true;
            try
            {
                if (process.HasExited) return true;
                GenerateConsoleCtrlEvent(CtrlCEvent, 0);
                return process.WaitForExit(timeoutMs);
            }
            catch { return process.HasExited; }
        }

        public static void KillBundledCoreIfRunning()
        {
            foreach (Process process in Process.GetProcessesByName(Path.GetFileNameWithoutExtension(ClientData.CorePath)))
            {
                using (process)
                {
                    try
                    {
                        string executable = process.MainModule.FileName;
                        if (String.Equals(Path.GetFullPath(executable), Path.GetFullPath(ClientData.CorePath),
                            StringComparison.OrdinalIgnoreCase))
                        {
                            process.Kill();
                            process.WaitForExit(5000);
                        }
                    }
                    catch { }
                }
            }
        }
    }

    internal static class SelfTest
    {
        private static string testLog;

        public static int Run()
        {
            string testDirectory = Path.Combine(Path.GetTempPath(), "RealityClient-selftest-" + Guid.NewGuid().ToString("N"));
            Process process = null;
            ProxySnapshot snapshot = null;
            bool snapshotSaved = false;
            try
            {
                Directory.CreateDirectory(ClientData.DirectoryPath);
                testLog = Path.Combine(ClientData.DirectoryPath, "self-test.log");
                File.WriteAllText(testLog, String.Empty);
                if (File.Exists(ClientData.ProxySnapshotPath))
                    throw new InvalidOperationException("Сначала восстановите предыдущую сессию клиента; тест не тронет её данные.");
                ClientData.Prepare();
                CoreRuntime.PrepareHiddenConsole();
                Directory.CreateDirectory(testDirectory);
                string linkFile = Path.Combine(testDirectory, "server.txt");
                string configFile = Path.Combine(testDirectory, "client.json");
                string link = "vless://00000000-0000-4000-8000-000000000000@127.0.0.1:1?encryption=none&security=reality&sni=example.com&pbk=uraIwXTmd3RJrfUjPSlgQyRK47ZXS5qP6JfnADzUMYI&sid=0123456789abcdef&type=tcp#SelfTest";
                ClientProfile protectedProfile = ClientData.ProtectProfile("SelfTest", link);
                Require(protectedProfile.ReadLink() == link, "DPAPI profile protection round-trip PASS");
                File.WriteAllText(linkFile, link, new UTF8Encoding(false));
                File.WriteAllText(configFile,
                    "{\"inbounds\":[{\"type\":\"mixed\",\"tag\":\"local\",\"listen\":\"127.0.0.1\",\"listen_port\":1080}]," +
                    "\"outbounds\":[{\"type\":\"vless\",\"tag\":\"proxy\",\"link_file\":\"server.txt\"}," +
                    "{\"type\":\"direct\",\"tag\":\"direct\"},{\"type\":\"block\",\"tag\":\"block\"}]," +
                    "\"route\":{\"rules\":[{\"action\":\"sniff\"}],\"final\":\"proxy\"}}",
                    new UTF8Encoding(false));
                Require(CoreRuntime.CheckConfiguration(configFile, 10000), "Config --check PASS");
                snapshot = ProxySnapshot.Capture();
                snapshot.Save(Path.Combine(testDirectory, "proxy-backup.dat"));
                snapshotSaved = true;
                process = CoreRuntime.Start(configFile, null, true);
                Require(CoreRuntime.WaitForListener(process, 12000), "Local mixed-proxy listener PASS");
                using (RegistryProxyProbe probe = new RegistryProxyProbe())
                    Require(probe.IsEnabledAt("127.0.0.1:1080"), "Windows system-proxy enabled PASS");
                using (TcpClient client = new TcpClient())
                {
                    client.Connect("127.0.0.1", 1080);
                    client.GetStream().Write(new byte[] { 5, 1, 0 }, 0, 3);
                    client.ReceiveTimeout = 3000;
                    byte[] reply = new byte[2];
                    int read = client.GetStream().Read(reply, 0, reply.Length);
                    Require(read == 2 && reply[0] == 5 && reply[1] == 0, "SOCKS5 no-auth handshake PASS");
                }
                Require(CoreRuntime.StopGracefully(process, 7000), "Graceful stop signal PASS");
                Require(process.ExitCode == 0, "Core graceful exit code PASS");
                process.Dispose(); process = null;
                Require(snapshot.RestoreIfClientOwnsProxy() || !IsAtClientProxy(), "Windows proxy restoration PASS");
                snapshotSaved = false;
                WriteLine("SELF_TEST=PASS");
                return 0;
            }
            catch (Exception ex)
            {
                WriteLine("SELF_TEST=FAIL");
                WriteLine(SafeException(ex));
                return 1;
            }
            finally
            {
                if (process != null)
                {
                    try
                    {
                        if (!CoreRuntime.StopGracefully(process, 5000)) process.Kill();
                    }
                    catch { try { process.Kill(); } catch { } }
                    process.Dispose();
                }
                if (snapshotSaved && snapshot != null)
                {
                    try { snapshot.RestoreIfClientOwnsProxy(); } catch { }
                }
                try { Directory.Delete(testDirectory, true); } catch { }
            }
        }

        private static bool IsAtClientProxy()
        {
            using (RegistryProxyProbe probe = new RegistryProxyProbe()) return probe.IsEnabledAt("127.0.0.1:1080");
        }

        private static void Require(string output, string label)
        {
            WriteLine(label);
            if (output != null && output.IndexOf("[секрет скрыт]", StringComparison.Ordinal) >= 0)
                throw new InvalidOperationException("В сообщении проверки обнаружен секрет.");
        }

        private static void Require(bool condition, string label)
        {
            WriteLine(label);
            if (!condition) throw new InvalidOperationException("Проверка не пройдена: " + label);
        }

        private static void WriteLine(string text)
        {
            if (!String.IsNullOrEmpty(testLog)) File.AppendAllText(testLog, text + Environment.NewLine, new UTF8Encoding(false));
        }

        private static string SafeException(Exception ex)
        {
            string value = ex.Message ?? "Unknown error";
            int secret = value.IndexOf("vless://", StringComparison.OrdinalIgnoreCase);
            if (secret >= 0) value = value.Substring(0, secret) + "[секрет скрыт]";
            return value;
        }
    }

    internal sealed class RegistryProxyProbe : IDisposable
    {
        private readonly Microsoft.Win32.RegistryKey key;
        public RegistryProxyProbe()
        {
            key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Internet Settings", false);
        }
        public bool IsEnabledAt(string address)
        {
            if (key == null) return false;
            return Convert.ToInt32(key.GetValue("ProxyEnable", 0)) == 1 &&
                String.Equals(Convert.ToString(key.GetValue("ProxyServer", "")), address, StringComparison.OrdinalIgnoreCase);
        }
        public void Dispose() { if (key != null) key.Dispose(); }
    }
}
