// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.IO;
using System.Net;
using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace RealityClientGui
{
    internal sealed class RegistryValueSnapshot
    {
        public bool Exists;
        public RegistryValueKind Kind;
        public object Value;
    }

    internal sealed class ProxySnapshot
    {
        private const string KeyPath = @"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
        private const string LocalProxy = "127.0.0.1:1080";
        private RegistryValueSnapshot enable;
        private RegistryValueSnapshot server;
        private RegistryValueSnapshot bypass;

        private ProxySnapshot(RegistryValueSnapshot e, RegistryValueSnapshot s, RegistryValueSnapshot b)
        {
            enable = e; server = s; bypass = b;
        }

        public static ProxySnapshot Capture()
        {
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(KeyPath, false))
            {
                if (key == null) throw new InvalidOperationException("Не удалось прочитать настройки прокси Windows.");
                RegistryValueSnapshot e = Read(key, "ProxyEnable");
                RegistryValueSnapshot s = Read(key, "ProxyServer");
                RegistryValueSnapshot b = Read(key, "ProxyOverride");
                ProxySnapshot snapshot = new ProxySnapshot(e, s, b);
                if (snapshot.IsEnabledAt(LocalProxy))
                    snapshot.enable = new RegistryValueSnapshot { Exists = true, Kind = RegistryValueKind.DWord, Value = 0 };
                return snapshot;
            }
        }

        private static RegistryValueSnapshot Read(RegistryKey key, string name)
        {
            try
            {
                RegistryValueKind kind = key.GetValueKind(name);
                object value = key.GetValue(name, null, RegistryValueOptions.DoNotExpandEnvironmentNames);
                return new RegistryValueSnapshot { Exists = value != null, Kind = kind, Value = value };
            }
            catch (IOException)
            {
                return new RegistryValueSnapshot { Exists = false };
            }
        }

        private bool IsEnabledAt(string address)
        {
            return enable.Exists && Convert.ToInt32(enable.Value) == 1 && server.Exists &&
                String.Equals(Convert.ToString(server.Value), address, StringComparison.OrdinalIgnoreCase);
        }

        public void Save(string path)
        {
            string temp = path + ".tmp";
            using (BinaryWriter writer = new BinaryWriter(File.Open(temp, FileMode.Create, FileAccess.Write, FileShare.None)))
            {
                writer.Write("RPROXY1");
                Write(writer, enable); Write(writer, server); Write(writer, bypass);
            }
            if (File.Exists(path)) File.Replace(temp, path, null); else File.Move(temp, path);
        }

        public static ProxySnapshot Load(string path)
        {
            using (BinaryReader reader = new BinaryReader(File.Open(path, FileMode.Open, FileAccess.Read, FileShare.Read)))
            {
                if (reader.ReadString() != "RPROXY1") throw new InvalidDataException("Файл восстановления прокси повреждён.");
                ProxySnapshot result = new ProxySnapshot(Read(reader), Read(reader), Read(reader));
                if (reader.BaseStream.Position != reader.BaseStream.Length)
                    throw new InvalidDataException("В файле восстановления прокси лишние данные.");
                return result;
            }
        }

        private static void Write(BinaryWriter writer, RegistryValueSnapshot value)
        {
            writer.Write(value.Exists);
            if (!value.Exists) return;
            writer.Write((int)value.Kind);
            if (value.Kind == RegistryValueKind.DWord) writer.Write(Convert.ToInt32(value.Value));
            else if (value.Kind == RegistryValueKind.QWord) writer.Write(Convert.ToInt64(value.Value));
            else writer.Write(Convert.ToString(value.Value) ?? String.Empty);
        }

        private static RegistryValueSnapshot Read(BinaryReader reader)
        {
            RegistryValueSnapshot result = new RegistryValueSnapshot { Exists = reader.ReadBoolean() };
            if (!result.Exists) return result;
            result.Kind = (RegistryValueKind)reader.ReadInt32();
            if (result.Kind == RegistryValueKind.DWord) result.Value = reader.ReadInt32();
            else if (result.Kind == RegistryValueKind.QWord) result.Value = reader.ReadInt64();
            else result.Value = reader.ReadString();
            return result;
        }

        public bool RestoreIfClientOwnsProxy()
        {
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(KeyPath, false))
            {
                if (key == null) return false;
                string currentServer = Convert.ToString(key.GetValue("ProxyServer", String.Empty));
                if (!String.Equals(currentServer, LocalProxy, StringComparison.OrdinalIgnoreCase)) return false;
            }
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(KeyPath, true))
            {
                if (key == null) return false;
                Restore(key, "ProxyEnable", enable);
                Restore(key, "ProxyServer", server);
                Restore(key, "ProxyOverride", bypass);
            }
            InternetSetOption(IntPtr.Zero, 39, IntPtr.Zero, 0); // SETTINGS_CHANGED
            InternetSetOption(IntPtr.Zero, 37, IntPtr.Zero, 0); // REFRESH
            return true;
        }

        private static void Restore(RegistryKey key, string name, RegistryValueSnapshot value)
        {
            if (!value.Exists) key.DeleteValue(name, false);
            else key.SetValue(name, value.Value, value.Kind);
        }

        [DllImport("wininet.dll", EntryPoint = "InternetSetOptionW", SetLastError = true)]
        private static extern bool InternetSetOption(IntPtr internet, int option, IntPtr buffer, int length);
    }
}
