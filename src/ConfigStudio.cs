// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.Drawing;
using System.IO;
using System.Text;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace RealityClientGui
{
    internal sealed class ConfigStudio : Form
    {
        private readonly TextBox editor = new TextBox();
        private readonly Label status = new Label();
        private string currentPath;
        public string SelectedConfigPath { get; private set; }

        public ConfigStudio()
        {
            Text = "Reality Client — полная конфигурация ядра";
            Size = new Size(1050, 760); MinimumSize = new Size(760, 540);
            StartPosition = FormStartPosition.CenterParent;
            BackColor = Color.FromArgb(12, 18, 23); ForeColor = Color.FromArgb(232, 239, 242);
            Font = new Font("Segoe UI", 9.5F);
            BuildUi();
            currentPath = ClientData.LoadAdvancedConfigPath();
            if (String.IsNullOrWhiteSpace(currentPath) || !File.Exists(currentPath))
            {
                currentPath = ClientData.AdvancedConfigPath;
                if (File.Exists(currentPath)) editor.Text = File.ReadAllText(currentPath, Encoding.UTF8);
                else editor.Text = StarterConfig;
            }
            else editor.Text = File.ReadAllText(currentPath, Encoding.UTF8);
            SelectedConfigPath = currentPath;
            status.Text = currentPath;
        }

        private const string StarterConfig = "{\r\n" +
            "  \"inbounds\": [{ \"type\": \"mixed\", \"tag\": \"local\", \"listen\": \"127.0.0.1\", \"listen_port\": 1080 }],\r\n" +
            "  \"outbounds\": [\r\n" +
            "    { \"type\": \"vless\", \"tag\": \"proxy\", \"link_file\": \"server.txt\" },\r\n" +
            "    { \"type\": \"direct\", \"tag\": \"direct\" },\r\n" +
            "    { \"type\": \"block\", \"tag\": \"block\" }\r\n" +
            "  ],\r\n" +
            "  \"route\": { \"final\": \"proxy\" }\r\n" +
            "}\r\n";

        private void BuildUi()
        {
            TableLayoutPanel root = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(14), ColumnCount = 1, RowCount = 4 };
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 44)); root.RowStyles.Add(new RowStyle(SizeType.Absolute, 48)); root.RowStyles.Add(new RowStyle(SizeType.Percent, 100)); root.RowStyles.Add(new RowStyle(SizeType.Absolute, 58));
            Controls.Add(root);
            FlowLayoutPanel bar = new FlowLayoutPanel { Dock = DockStyle.Fill, WrapContents = false };
            AddButton(bar, "Открыть…", OpenConfig); AddButton(bar, "Сохранить", SaveClicked); AddButton(bar, "Сохранить как…", SaveAsConfig);
            AddButton(bar, "Проверить ядром", ValidateConfig);
            root.Controls.Add(bar, 0, 0);
            editor.Dock = DockStyle.Fill; editor.Multiline = true; editor.AcceptsTab = true; editor.WordWrap = false;
            editor.ScrollBars = ScrollBars.Both; editor.Font = new Font("Consolas", 10F);
            editor.BackColor = Color.FromArgb(10, 16, 21); editor.ForeColor = Color.FromArgb(215, 230, 235);
            editor.BorderStyle = BorderStyle.FixedSingle;
            Label help = new Label { Text = "Полный JSON-конфиг sing-box или Xray: VLESS/Trojan, TLS/REALITY, transport, группы, subscriptions, routing, DNS, TUN, API и другие настройки ядра. Ссылочные файлы (link_file, rule sets, сертификаты) держите рядом с конфигом. Секреты внутри самого JSON сохраняются открытым текстом — используйте link_file и ограничьте доступ к файлам.",
                Dock = DockStyle.Fill, ForeColor = Color.FromArgb(255, 210, 122), BackColor = Color.FromArgb(17, 26, 33), Padding = new Padding(8) };
            root.Controls.Add(help, 0, 1);
            root.Controls.Add(editor, 0, 2);
            TableLayoutPanel footer = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1 };
            footer.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100)); footer.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 150));
            status.Dock = DockStyle.Fill; status.TextAlign = ContentAlignment.MiddleLeft; status.ForeColor = Color.FromArgb(145, 169, 178); status.AutoEllipsis = true;
            footer.Controls.Add(status, 0, 0);
            Button use = new Button { Text = "Использовать", Dock = DockStyle.Fill, FlatStyle = FlatStyle.Flat, BackColor = Color.FromArgb(23, 126, 112), ForeColor = ForeColor };
            use.Click += delegate { if (SaveConfig(null, null)) { SelectedConfigPath = currentPath; ClientData.SaveAdvancedConfigPath(currentPath); DialogResult = DialogResult.OK; Close(); } };
            footer.Controls.Add(use, 1, 0); root.Controls.Add(footer, 0, 3);
        }

        private void AddButton(FlowLayoutPanel bar, string text, EventHandler handler)
        {
            Button button = new Button { Text = text, Width = 125, Height = 32, FlatStyle = FlatStyle.Flat, BackColor = Color.FromArgb(31, 44, 52), ForeColor = ForeColor, Margin = new Padding(0, 0, 8, 0) };
            button.Click += handler; bar.Controls.Add(button);
        }

        private void OpenConfig(object sender, EventArgs e)
        {
            using (OpenFileDialog dialog = new OpenFileDialog { Filter = "Конфигурации JSON|*.json;*.jsonc|Все файлы|*.*", CheckFileExists = true })
                if (dialog.ShowDialog(this) == DialogResult.OK) { currentPath = dialog.FileName; editor.Text = File.ReadAllText(currentPath, Encoding.UTF8); status.Text = currentPath; }
        }

        private bool SaveConfig(object sender, EventArgs e)
        {
            try
            {
                string directory = Path.GetDirectoryName(Path.GetFullPath(currentPath));
                if (!Directory.Exists(directory)) Directory.CreateDirectory(directory);
                File.WriteAllText(currentPath, editor.Text, new UTF8Encoding(false));
                status.Text = "Сохранено: " + currentPath; return true;
            }
            catch (Exception ex) { MessageBox.Show(this, ex.Message, "Сохранение не удалось", MessageBoxButtons.OK, MessageBoxIcon.Error); return false; }
        }

        private void SaveClicked(object sender, EventArgs e) { SaveConfig(null, null); }

        private void SaveAsConfig(object sender, EventArgs e)
        {
            using (SaveFileDialog dialog = new SaveFileDialog { Filter = "Конфигурация JSON|*.json|Все файлы|*.*", DefaultExt = "json", FileName = Path.GetFileName(currentPath) })
                if (dialog.ShowDialog(this) == DialogResult.OK) { currentPath = dialog.FileName; SaveConfig(null, null); }
        }

        private async void ValidateConfig(object sender, EventArgs e)
        {
            if (!SaveConfig(null, null)) return;
            status.Text = "Проверка конфигурации самим ядром…";
            try
            {
                string output = await Task.Run(delegate { return CoreRuntime.CheckConfiguration(currentPath, 20000); });
                status.Text = String.IsNullOrWhiteSpace(output) ? "Проверка ядром пройдена." : output.Trim();
                MessageBox.Show(this, "Ядро приняло конфигурацию. Это проверка структуры и параметров, не проверка соединения с сервером.", "Конфигурация корректна", MessageBoxButtons.OK, MessageBoxIcon.Information);
            }
            catch (Exception ex) { status.Text = "Проверка не пройдена."; MessageBox.Show(this, ex.Message, "Конфигурация отклонена ядром", MessageBoxButtons.OK, MessageBoxIcon.Warning); }
        }
    }
}
