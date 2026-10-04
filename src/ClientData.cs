// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Security.AccessControl;
using System.Security.Principal;

namespace RealityClientGui
{
    internal sealed class ClientProfile
    {
        public string Name;
        public byte[] ProtectedLink;

        public string ReadLink()
        {
            byte[] clear = ProtectedData.Unprotect(ProtectedLink, null, DataProtectionScope.CurrentUser);
            try { return Encoding.UTF8.GetString(clear); }
            finally { Array.Clear(clear, 0, clear.Length); }
        }
    }

    internal static class ClientData
    {
        private const string Header = "RCLIENT1";
        public static readonly string DirectoryPath = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "RealityClient");
        public static readonly string VaultPath = Path.Combine(DirectoryPath, "profiles.dat");
        public static readonly string CorePath = Path.Combine(DirectoryPath, "reality-client.exe");
        public static readonly string ConfigPath = Path.Combine(DirectoryPath, "client.json");
        public static readonly string AdvancedConfigPath = Path.Combine(DirectoryPath, "advanced-config.json");
        public static readonly string SettingsPath = Path.Combine(DirectoryPath, "settings.txt");
        public static readonly string LinkPath = Path.Combine(DirectoryPath, "server.txt");
        public static readonly string ProxySnapshotPath = Path.Combine(DirectoryPath, "proxy-backup.dat");

        public static void Prepare()
        {
            Directory.CreateDirectory(DirectoryPath);
            HardenDirectory();
            if (File.Exists(LinkPath))
            {
                try { File.Delete(LinkPath); } catch { }
            }
            ExtractCore();
            WriteConfig();
        }

        public static string LoadAdvancedConfigPath()
        {
            if (!File.Exists(SettingsPath)) return String.Empty;
            try { return File.ReadAllText(SettingsPath, Encoding.UTF8).Trim(); }
            catch { return String.Empty; }
        }

        public static void SaveAdvancedConfigPath(string path)
        {
            File.WriteAllText(SettingsPath, path ?? String.Empty, new UTF8Encoding(false));
        }

        private static void HardenDirectory()
        {
            DirectoryInfo info = new DirectoryInfo(DirectoryPath);
            DirectorySecurity security = info.GetAccessControl();
            security.SetAccessRuleProtection(true, false);
            SecurityIdentifier user = WindowsIdentity.GetCurrent().User;
            SecurityIdentifier system = new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null);
            SecurityIdentifier admins = new SecurityIdentifier(WellKnownSidType.BuiltinAdministratorsSid, null);
            FileSystemRights rights = FileSystemRights.FullControl;
            InheritanceFlags inherit = InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit;
            security.AddAccessRule(new FileSystemAccessRule(user, rights, inherit,
                PropagationFlags.None, AccessControlType.Allow));
            security.AddAccessRule(new FileSystemAccessRule(system, rights, inherit,
                PropagationFlags.None, AccessControlType.Allow));
            security.AddAccessRule(new FileSystemAccessRule(admins, rights, inherit,
                PropagationFlags.None, AccessControlType.Allow));
            info.SetAccessControl(security);
        }

        private static void ExtractCore()
        {
            string resource = "RealityClientGui.reality-client.exe";
            using (Stream input = typeof(ClientData).Assembly.GetManifestResourceStream(resource))
            {
                if (input == null) throw new InvalidOperationException("В приложении не найден встроенный файл ядра.");
                byte[] bundled;
                using (MemoryStream memory = new MemoryStream())
                {
                    input.CopyTo(memory);
                    bundled = memory.ToArray();
                }
                if (File.Exists(CorePath))
                {
                    byte[] installed = File.ReadAllBytes(CorePath);
                    using (SHA256 sha = SHA256.Create())
                    {
                        if (Convert.ToBase64String(sha.ComputeHash(installed)) == Convert.ToBase64String(sha.ComputeHash(bundled)))
                            return;
                    }
                }
                string temporary = CorePath + ".new";
                try
                {
                    using (FileStream output = new FileStream(temporary, FileMode.Create, FileAccess.Write, FileShare.None))
                    {
                        output.Write(bundled, 0, bundled.Length);
                        output.Flush(true);
                    }
                    if (File.Exists(CorePath)) File.Replace(temporary, CorePath, null);
                    else File.Move(temporary, CorePath);
                }
                finally { if (File.Exists(temporary)) try { File.Delete(temporary); } catch { } }
            }
        }

        private static void WriteConfig()
        {
            string config =
                "{\r\n" +
                "  \"inbounds\": [{ \"type\": \"mixed\", \"tag\": \"local\", \"listen\": \"127.0.0.1\", \"listen_port\": 1080 }],\r\n" +
                "  \"outbounds\": [\r\n" +
                "    { \"type\": \"vless\", \"tag\": \"proxy\", \"link_file\": \"server.txt\" },\r\n" +
                "    { \"type\": \"direct\", \"tag\": \"direct\" },\r\n" +
                "    { \"type\": \"block\", \"tag\": \"block\" }\r\n" +
                "  ],\r\n" +
                "  \"route\": { \"rules\": [{ \"action\": \"sniff\" }], \"final\": \"proxy\" }\r\n" +
                "}\r\n";
            File.WriteAllText(ConfigPath, config, new UTF8Encoding(false));
        }

        public static List<ClientProfile> LoadProfiles()
        {
            List<ClientProfile> profiles = new List<ClientProfile>();
            if (!File.Exists(VaultPath)) return profiles;
            using (BinaryReader reader = new BinaryReader(File.Open(VaultPath, FileMode.Open, FileAccess.Read, FileShare.Read)))
            {
                if (reader.ReadString() != Header) throw new InvalidDataException("Неизвестный формат хранилища профилей.");
                int count = reader.ReadInt32();
                if (count < 0 || count > 100) throw new InvalidDataException("Некорректное количество профилей.");
                for (int i = 0; i < count; i++)
                {
                    string name = reader.ReadString();
                    int size = reader.ReadInt32();
                    if (size < 1 || size > 16384) throw new InvalidDataException("Некорректный размер профиля.");
                    byte[] encrypted = reader.ReadBytes(size);
                    if (encrypted.Length != size) throw new EndOfStreamException("Файл профилей повреждён.");
                    profiles.Add(new ClientProfile { Name = name, ProtectedLink = encrypted });
                }
                if (reader.BaseStream.Position != reader.BaseStream.Length)
                    throw new InvalidDataException("В хранилище обнаружены лишние данные.");
            }
            return profiles;
        }

        public static void SaveProfiles(IList<ClientProfile> profiles)
        {
            string temporary = VaultPath + ".tmp";
            using (BinaryWriter writer = new BinaryWriter(File.Open(temporary, FileMode.Create, FileAccess.Write, FileShare.None)))
            {
                writer.Write(Header);
                writer.Write(profiles.Count);
                foreach (ClientProfile profile in profiles)
                {
                    writer.Write(profile.Name);
                    writer.Write(profile.ProtectedLink.Length);
                    writer.Write(profile.ProtectedLink);
                }
            }
            if (File.Exists(VaultPath)) File.Replace(temporary, VaultPath, null);
            else File.Move(temporary, VaultPath);
        }

        public static ClientProfile ProtectProfile(string name, string link)
        {
            byte[] clear = Encoding.UTF8.GetBytes(link);
            try
            {
                return new ClientProfile
                {
                    Name = name,
                    ProtectedLink = ProtectedData.Protect(clear, null, DataProtectionScope.CurrentUser)
                };
            }
            finally { Array.Clear(clear, 0, clear.Length); }
        }

        public static void WriteSessionLink(string link)
        {
            File.WriteAllText(LinkPath, link + "\r\n", new UTF8Encoding(false));
        }

        public static void DeleteSessionLink()
        {
            if (!File.Exists(LinkPath)) return;
            try
            {
                long length = new FileInfo(LinkPath).Length;
                using (FileStream stream = new FileStream(LinkPath, FileMode.Open, FileAccess.Write, FileShare.None))
                {
                    byte[] zeros = new byte[4096];
                    long written = 0;
                    while (written < length)
                    {
                        int n = (int)Math.Min(zeros.Length, length - written);
                        stream.Write(zeros, 0, n);
                        written += n;
                    }
                    stream.Flush(true);
                }
            }
            catch { }
            try { File.Delete(LinkPath); } catch { }
        }
    }
}
