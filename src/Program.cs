// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.Threading;
using System.Windows.Forms;

namespace RealityClientGui
{
    internal static class Program
    {
        private static Mutex instanceMutex;

        [STAThread]
        private static int Main(string[] args)
        {
            try
            {
                if (args.Length == 1 && args[0] == "--self-test")
                    return SelfTest.Run();

                bool created;
                string sid = System.Security.Principal.WindowsIdentity.GetCurrent().User.Value;
                instanceMutex = new Mutex(true, "Local\\RealityClientGui_" + sid, out created);
                if (!created)
                {
                    MessageBox.Show("Reality Client уже запущен.", "Reality Client",
                        MessageBoxButtons.OK, MessageBoxIcon.Information);
                    return 0;
                }

                Application.EnableVisualStyles();
                Application.SetCompatibleTextRenderingDefault(false);
                Application.Run(new MainForm());
                return 0;
            }
            catch (Exception ex)
            {
                MessageBox.Show("Клиент не удалось запустить:\r\n" + ex.Message,
                    "Reality Client", MessageBoxButtons.OK, MessageBoxIcon.Error);
                return 1;
            }
            finally
            {
                if (instanceMutex != null)
                {
                    try { instanceMutex.ReleaseMutex(); } catch { }
                    instanceMutex.Dispose();
                }
            }
        }
    }
}
