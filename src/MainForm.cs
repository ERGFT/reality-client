// SPDX-License-Identifier: GPL-3.0-or-later
using Microsoft.Win32;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Net;
using System.Text;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace RealityClientGui
{
    internal sealed class MainForm : Form
    {
        private readonly List<ClientProfile> profiles = new List<ClientProfile>();
        private readonly ComboBox profilePicker = new ComboBox();
        private readonly TextBox nameInput = new TextBox();
        private readonly TextBox linkInput = new TextBox();
        private readonly Button showLink = new Button();
        private readonly Label stateLabel = new Label();
        private readonly Label endpointLabel = new Label();
        private readonly Label warningLabel = new Label();
        private readonly Button connectButton = new Button();
        private readonly Button recoveryButton = new Button();
        private readonly CheckBox advancedMode = new CheckBox();
        private readonly CheckBox systemProxyMode = new CheckBox();
        private readonly Button configButton = new Button();
        private readonly RichTextBox activityLog = new RichTextBox();
        private readonly Timer monitor = new Timer();
        private Process coreProcess;
        private bool loadingProfile;
        private bool stopping;
        private bool starting;
        private bool allowClose;
        private bool closeAfterStart;
        private bool closeAfterStop;

        public MainForm()
        {
            Text = "Reality Client";
            MinimumSize = new Size(760, 680);
            Size = new Size(880, 760);
            StartPosition = FormStartPosition.CenterScreen;
            BackColor = Color.FromArgb(12, 18, 23);
            ForeColor = Color.FromArgb(232, 239, 242);
            Font = new Font("Segoe UI", 10F);
            BuildInterface();
            ClientData.Prepare();
            CoreRuntime.PrepareHiddenConsole();
            LoadSavedProfiles();
            UpdateRecoveryState();
            monitor.Interval = 500;
            monitor.Tick += MonitorTick;
            monitor.Start();
            FormClosing += ClosingForm;
            AddLog("Готово. Профили шифруются средствами Windows DPAPI.");
        }

        private void BuildInterface()
        {
            TableLayoutPanel root = new TableLayoutPanel();
            root.Dock = DockStyle.Fill;
            root.Padding = new Padding(26);
            root.ColumnCount = 1;
            root.RowCount = 5;
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 74));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 205));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 270));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 52));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            Controls.Add(root);

            Panel header = new Panel { Dock = DockStyle.Fill };
            Label brand = new Label
            {
                Text = "REALITY  /  CLIENT",
                Font = new Font("Segoe UI Semibold", 18F, FontStyle.Bold),
                ForeColor = Color.FromArgb(85, 214, 190),
                AutoSize = true,
                Location = new Point(0, 10)
            };
            stateLabel.Text = "Отключено";
            stateLabel.AutoSize = true;
            stateLabel.ForeColor = Color.FromArgb(174, 190, 198);
            stateLabel.Anchor = AnchorStyles.Top | AnchorStyles.Right;
            stateLabel.Location = new Point(Width - 230, 23);
            header.Controls.Add(brand); header.Controls.Add(stateLabel);
            header.Resize += delegate { stateLabel.Location = new Point(header.ClientSize.Width - stateLabel.Width - 8, 23); };
            root.Controls.Add(header, 0, 0);

            Panel profileCard = CardPanel();
            profileCard.Padding = new Padding(18);
            Label profileTitle = Caption("Профиль подключения");
            profileTitle.Location = new Point(18, 13);
            profileCard.Controls.Add(profileTitle);
            profilePicker.DropDownStyle = ComboBoxStyle.DropDownList;
            profilePicker.Location = new Point(18, 48);
            profilePicker.Width = 355;
            profilePicker.Anchor = AnchorStyles.Top | AnchorStyles.Left;
            profilePicker.BackColor = Color.FromArgb(18, 27, 34);
            profilePicker.ForeColor = ForeColor;
            profilePicker.SelectedIndexChanged += ProfileSelected;
            profileCard.Controls.Add(profilePicker);
            Button deleteButton = ButtonStyle("Удалить", false);
            deleteButton.Location = new Point(384, 46);
            deleteButton.Anchor = AnchorStyles.Top | AnchorStyles.Left;
            deleteButton.Width = 104;
            deleteButton.Click += DeleteProfile;
            profileCard.Controls.Add(deleteButton);
            Label nameCaption = Caption("Название"); nameCaption.Location = new Point(18, 88);
            profileCard.Controls.Add(nameCaption);
            nameInput.Location = new Point(18, 111); nameInput.Width = 220;
            nameInput.Anchor = AnchorStyles.Top | AnchorStyles.Left;
            nameInput.AccessibleName = "Название профиля";
            StyleInput(nameInput); profileCard.Controls.Add(nameInput);
            Label linkCaption = Caption("Ссылка VLESS"); linkCaption.Location = new Point(253, 88);
            linkCaption.Anchor = AnchorStyles.Top | AnchorStyles.Left;
            profileCard.Controls.Add(linkCaption);
            linkInput.Location = new Point(253, 111); linkInput.Width = 235;
            linkInput.Anchor = AnchorStyles.Top | AnchorStyles.Left;
            linkInput.AccessibleName = "Ссылка VLESS";
            linkInput.UseSystemPasswordChar = true;
            linkInput.ShortcutsEnabled = true;
            linkInput.KeyDown += LinkInputKeyDown;
            StyleInput(linkInput); profileCard.Controls.Add(linkInput);
            showLink.Text = "Показать"; showLink.Size = new Size(90, 29);
            showLink.Location = new Point(253, 148); showLink.Anchor = AnchorStyles.Top | AnchorStyles.Left;
            StyleButton(showLink, false);
            showLink.Click += delegate
            {
                linkInput.UseSystemPasswordChar = !linkInput.UseSystemPasswordChar;
                showLink.Text = linkInput.UseSystemPasswordChar ? "Показать" : "Скрыть";
            };
            profileCard.Controls.Add(showLink);
            Button pasteLink = ButtonStyle("Вставить", false);
            pasteLink.Size = new Size(90, 29); pasteLink.Location = new Point(353, 148);
            pasteLink.Anchor = AnchorStyles.Top | AnchorStyles.Left;
            pasteLink.Click += PasteLinkFromClipboard;
            profileCard.Controls.Add(pasteLink);
            ContextMenuStrip linkMenu = new ContextMenuStrip();
            ToolStripMenuItem pasteMenuItem = new ToolStripMenuItem("Вставить ссылку");
            pasteMenuItem.Click += PasteLinkFromClipboard;
            linkMenu.Items.Add(pasteMenuItem);
            linkInput.ContextMenuStrip = linkMenu;
            Button saveButton = ButtonStyle("Сохранить профиль", true);
            saveButton.Location = new Point(18, 148); saveButton.Width = 220;
            saveButton.Click += SaveProfile;
            profileCard.Controls.Add(saveButton);
            root.Controls.Add(profileCard, 0, 1);

            Panel connectionCard = CardPanel(); connectionCard.Padding = new Padding(20);
            Label connectTitle = Caption("Подключение"); connectTitle.Location = new Point(20, 15);
            connectionCard.Controls.Add(connectTitle);
            endpointLabel.Text = "Локальный прокси: 127.0.0.1:1080";
            endpointLabel.Location = new Point(20, 51); endpointLabel.AutoSize = true;
            endpointLabel.ForeColor = Color.FromArgb(186, 201, 208);
            connectionCard.Controls.Add(endpointLabel);
            connectButton.Text = "Подключить"; connectButton.Size = new Size(190, 48);
            connectButton.Location = new Point(20, 91); connectButton.Click += ConnectClicked;
            StyleButton(connectButton, true); connectionCard.Controls.Add(connectButton);
            recoveryButton.Text = "Восстановить прокси";
            recoveryButton.Size = new Size(205, 42); recoveryButton.Location = new Point(224, 94);
            recoveryButton.Click += RecoverClicked; StyleButton(recoveryButton, false);
            recoveryButton.Visible = false; connectionCard.Controls.Add(recoveryButton);
            advancedMode.Text = "Использовать полный конфиг ядра"; advancedMode.AutoSize = true;
            advancedMode.Location = new Point(20, 151); advancedMode.ForeColor = ForeColor;
            advancedMode.Checked = !String.IsNullOrWhiteSpace(ClientData.LoadAdvancedConfigPath());
            advancedMode.CheckedChanged += delegate { if (advancedMode.Checked) systemProxyMode.Checked = false; UpdateConfigMode(); };
            connectionCard.Controls.Add(advancedMode);
            configButton.Text = "Настройки…"; configButton.Size = new Size(140, 32); configButton.Location = new Point(280, 146);
            StyleButton(configButton, false); configButton.Click += EditAdvancedConfig; connectionCard.Controls.Add(configButton);
            systemProxyMode.Text = "Включить системный прокси Windows"; systemProxyMode.AutoSize = true;
            systemProxyMode.Checked = !advancedMode.Checked; systemProxyMode.Location = new Point(20, 185); systemProxyMode.ForeColor = ForeColor;
            connectionCard.Controls.Add(systemProxyMode);
            warningLabel.Text = "Системный прокси не перехватывает приложения, которые его игнорируют. Полный конфиг даёт доступ к функциям ядра; TUN требует подходящей ОС, драйвера/прав и корректной настройки.";
            warningLabel.Location = new Point(20, 218); warningLabel.AutoSize = true;
            warningLabel.MaximumSize = new Size(750, 0);
            warningLabel.ForeColor = Color.FromArgb(255, 210, 122);
            connectionCard.Controls.Add(warningLabel);
            UpdateConfigMode();
            root.Controls.Add(connectionCard, 0, 2);

            Panel logHeader = new Panel { Dock = DockStyle.Fill };
            Label activity = Caption("Состояние клиента"); activity.Location = new Point(2, 14);
            logHeader.Controls.Add(activity);
            Label privacy = new Label { Text = "Секретные ссылки не попадают в журнал", AutoSize = true,
                ForeColor = Color.FromArgb(132, 153, 162), Anchor = AnchorStyles.Top | AnchorStyles.Right };
            privacy.Location = new Point(Width - 390, 16);
            logHeader.Resize += delegate { privacy.Location = new Point(logHeader.ClientSize.Width - privacy.Width - 4, 16); };
            logHeader.Controls.Add(privacy);
            root.Controls.Add(logHeader, 0, 3);
            activityLog.Dock = DockStyle.Fill;
            activityLog.ReadOnly = true;
            activityLog.BackColor = Color.FromArgb(15, 23, 29);
            activityLog.ForeColor = Color.FromArgb(191, 207, 213);
            activityLog.BorderStyle = BorderStyle.FixedSingle;
            activityLog.Font = new Font("Consolas", 9.5F);
            root.Controls.Add(activityLog, 0, 4);
        }

        private Panel CardPanel()
        {
            return new Panel { Dock = DockStyle.Fill, BackColor = Color.FromArgb(17, 26, 33),
                BorderStyle = BorderStyle.FixedSingle, Margin = new Padding(0, 0, 0, 12) };
        }

        private Label Caption(string text)
        {
            return new Label { Text = text, AutoSize = true, ForeColor = Color.FromArgb(146, 166, 175),
                Font = new Font("Segoe UI Semibold", 9.5F, FontStyle.Bold) };
        }

        private void StyleInput(TextBox box)
        {
            box.Height = 28;
            box.BorderStyle = BorderStyle.FixedSingle;
            box.BackColor = Color.FromArgb(10, 16, 21);
            box.ForeColor = ForeColor;
            box.AccessibleRole = AccessibleRole.Text;
        }

        private Button ButtonStyle(string text, bool primary)
        {
            Button button = new Button { Text = text, FlatStyle = FlatStyle.Flat, ForeColor = ForeColor,
                BackColor = primary ? Color.FromArgb(23, 126, 112) : Color.FromArgb(31, 44, 52),
                Cursor = Cursors.Hand, Height = 34 };
            button.FlatAppearance.BorderColor = primary ? Color.FromArgb(52, 172, 151) : Color.FromArgb(55, 72, 81);
            return button;
        }

        private void StyleButton(Button button, bool primary)
        {
            button.FlatStyle = FlatStyle.Flat;
            button.FlatAppearance.BorderColor = primary ? Color.FromArgb(52, 172, 151) : Color.FromArgb(55, 72, 81);
            button.BackColor = primary ? Color.FromArgb(23, 126, 112) : Color.FromArgb(31, 44, 52);
            button.ForeColor = ForeColor; button.Cursor = Cursors.Hand;
            button.Font = new Font("Segoe UI Semibold", 10F, FontStyle.Bold);
            button.AccessibleRole = AccessibleRole.PushButton;
        }

        private void LoadSavedProfiles()
        {
            try
            {
                profiles.AddRange(ClientData.LoadProfiles());
                RefreshPicker(-1);
                if (profiles.Count > 0) profilePicker.SelectedIndex = 0;
            }
            catch (Exception ex)
            {
                AddLog("Не удалось прочитать защищённые профили: " + ex.Message);
                MessageBox.Show("Хранилище профилей не удалось открыть. Существующие данные оставлены без изменений.\r\n\r\n" + ex.Message,
                    "Профили", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }

        private void RefreshPicker(int select)
        {
            loadingProfile = true;
            profilePicker.Items.Clear();
            foreach (ClientProfile profile in profiles) profilePicker.Items.Add(profile.Name);
            if (select >= 0 && select < profilePicker.Items.Count) profilePicker.SelectedIndex = select;
            else if (profilePicker.Items.Count == 0) profilePicker.SelectedIndex = -1;
            loadingProfile = false;
        }

        private void ProfileSelected(object sender, EventArgs e)
        {
            if (loadingProfile || profilePicker.SelectedIndex < 0) return;
            try
            {
                ClientProfile profile = profiles[profilePicker.SelectedIndex];
                nameInput.Text = profile.Name;
                linkInput.Text = profile.ReadLink();
                AddLog("Выбран профиль «" + profile.Name + "».");
            }
            catch (Exception ex)
            {
                linkInput.Clear();
                AddLog("Не удалось расшифровать профиль. Секрет не восстановлен.");
                MessageBox.Show("Windows не смогла расшифровать ссылку профиля. Можно удалить профиль и добавить его снова.\r\n\r\n" + ex.Message,
                    "Профиль недоступен", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }

        private void SaveProfile(object sender, EventArgs e)
        {
            string name = nameInput.Text.Trim();
            string link = linkInput.Text.Trim();
            string problem = ValidateLink(link);
            if (problem != null)
            {
                MessageBox.Show(problem, "Проверьте ссылку", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }
            if (String.IsNullOrWhiteSpace(name)) name = "Профиль " + DateTime.Now.ToString("dd.MM HH:mm");
            try
            {
                ClientProfile protectedProfile = ClientData.ProtectProfile(name, link);
                int selected = profilePicker.SelectedIndex;
                if (selected >= 0 && selected < profiles.Count) profiles[selected] = protectedProfile;
                else { profiles.Add(protectedProfile); selected = profiles.Count - 1; }
                ClientData.SaveProfiles(profiles);
                RefreshPicker(selected);
                AddLog("Профиль «" + name + "» сохранён с шифрованием DPAPI.");
            }
            catch (Exception ex)
            {
                AddLog("Сохранить профиль не удалось: " + ex.Message);
                MessageBox.Show("Профиль не сохранён.\r\n\r\n" + ex.Message,
                    "Ошибка сохранения", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }

        private void DeleteProfile(object sender, EventArgs e)
        {
            int selected = profilePicker.SelectedIndex;
            if (selected < 0 || selected >= profiles.Count) return;
            if (coreProcess != null || starting || stopping)
            {
                MessageBox.Show("Сначала отключите клиент.", "Профиль занят", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            string name = profiles[selected].Name;
            if (MessageBox.Show("Удалить профиль «" + name + "»?", "Удаление профиля",
                MessageBoxButtons.YesNo, MessageBoxIcon.Question) != DialogResult.Yes) return;
            profiles.RemoveAt(selected);
            try
            {
                ClientData.SaveProfiles(profiles);
                RefreshPicker(profiles.Count == 0 ? -1 : Math.Min(selected, profiles.Count - 1));
                if (profiles.Count == 0) { nameInput.Clear(); linkInput.Clear(); }
                AddLog("Профиль удалён.");
            }
            catch (Exception ex)
            {
                AddLog("Удалить профиль не удалось: " + ex.Message);
                MessageBox.Show("Не удалось сохранить изменения хранилища.\r\n\r\n" + ex.Message,
                    "Ошибка удаления", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }

        private async void ConnectClicked(object sender, EventArgs e)
        {
            if (coreProcess != null)
            {
                await StopCoreAsync();
                return;
            }
            bool custom = advancedMode.Checked;
            string link = linkInput.Text.Trim();
            string problem = custom ? null : ValidateLink(link);
            string configPath = custom ? ClientData.LoadAdvancedConfigPath() : ClientData.ConfigPath;
            if (custom && (String.IsNullOrWhiteSpace(configPath) || !File.Exists(configPath)))
                problem = "Сначала откройте и сохраните полный конфиг в разделе «Настройки…».";
            if (problem != null)
            {
                MessageBox.Show(problem, "Проверьте ссылку", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }
            if (File.Exists(ClientData.ProxySnapshotPath))
            {
                MessageBox.Show("Найдено незавершённое восстановление системного прокси. Сначала нажмите «Восстановить прокси».",
                    "Нужно восстановление", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }

            starting = true;
            SetBusy(true, "Проверка конфигурации…");
            string failure = null;
            try
            {
                if (!custom)
                {
                    string profileName = String.IsNullOrWhiteSpace(nameInput.Text) ? "Текущий профиль" : nameInput.Text.Trim();
                    ClientProfile profile = ClientData.ProtectProfile(profileName, link);
                    int selected = profilePicker.SelectedIndex;
                    if (selected >= 0 && selected < profiles.Count) profiles[selected] = profile;
                    else { profiles.Add(profile); selected = profiles.Count - 1; }
                    ClientData.SaveProfiles(profiles); RefreshPicker(selected);
                    ClientData.WriteSessionLink(link);
                }
                await Task.Run(delegate { CoreRuntime.CheckConfiguration(configPath, 20000); });
                if (systemProxyMode.Checked)
                {
                    ProxySnapshot snapshot = ProxySnapshot.Capture();
                    snapshot.Save(ClientData.ProxySnapshotPath);
                }
                AddLog("Конфигурация проверена ядром.");
                coreProcess = CoreRuntime.Start(configPath, CoreLog, systemProxyMode.Checked);
                bool listening = custom
                    ? await Task.Run(delegate { return CoreRuntime.WaitForProcessReady(coreProcess, 3000); })
                    : await Task.Run(delegate { return CoreRuntime.WaitForListener(coreProcess, 15000); });
                if (!listening)
                {
                    failure = coreProcess.HasExited ? "Ядро завершилось при запуске." : "Локальный порт 1080 не открылся.";
                    AddLog(failure);
                }
                else
                {
                    SetBusy(false, "Прокси запущен");
                    connectButton.Text = "Отключить";
                    AddLog(custom ? "Ядро запущено по полному конфигу. Доступность всех входов проверяйте согласно его настройкам." : "Локальный прокси работает на 127.0.0.1:1080.");
                    if (systemProxyMode.Checked) AddLog("Системный прокси включён; соединение с удалённым сервером проверяется реальным запросом.");
                    UpdateRecoveryState();
                }
            }
            catch (Exception ex)
            {
                AddLog("Запуск не удался: " + SafeMessage(ex.Message));
                failure = SafeMessage(ex.Message);
            }
            if (failure != null)
                await StopCoreAsync();

            starting = false;
            if (closeAfterStart)
            {
                closeAfterStart = false;
                if (coreProcess != null || File.Exists(ClientData.ProxySnapshotPath))
                    await StopCoreAsync();
                allowClose = true;
                Close();
                return;
            }
            if (failure != null)
            {
                MessageBox.Show("Подключение не запущено. Проверьте формат и поддержку параметров ссылки.\r\n\r\n" + failure,
                    "Не удалось подключиться", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }

        private async Task StopCoreAsync()
        {
            if (stopping) return;
            stopping = true;
            SetBusy(true, "Остановка ядра…");
            Process process = coreProcess;
            bool stopped = true;
            try
            {
                if (process != null)
                {
                    stopped = await Task.Run(delegate { return CoreRuntime.StopGracefully(process, 7000); });
                    if (!stopped)
                    {
                        try { process.Kill(); } catch { }
                        try { process.WaitForExit(5000); } catch { }
                        if (File.Exists(ClientData.ProxySnapshotPath))
                            try { await Task.Run(delegate { CoreRuntime.DisableSystemProxy(7000); }); } catch { }
                        AddLog("Штатная остановка не ответила; выполнено аварийное отключение системного прокси.");
                    }
                }
                ProxySnapshot snapshot = null;
                if (File.Exists(ClientData.ProxySnapshotPath))
                {
                    try
                    {
                        snapshot = ProxySnapshot.Load(ClientData.ProxySnapshotPath);
                        if (snapshot.RestoreIfClientOwnsProxy()) AddLog("Исходные настройки системного прокси восстановлены.");
                        File.Delete(ClientData.ProxySnapshotPath);
                    }
                    catch (Exception ex)
                    {
                        AddLog("Прокси не удалось восстановить: " + SafeMessage(ex.Message));
                    }
                }
                if (process != null) { process.Dispose(); coreProcess = null; }
                ClientData.DeleteSessionLink();
                SetBusy(false, stopped ? "Отключено" : "Ядро остановлено принудительно");
                connectButton.Text = "Подключить";
                AddLog("Клиент отключён.");
                UpdateRecoveryState();
            }
            finally
            {
                stopping = false;
                if (!allowClose) SetBusy(false, coreProcess == null ? "Отключено" : "Подключено");
                if (closeAfterStop && !IsDisposed)
                {
                    closeAfterStop = false;
                    allowClose = true;
                    BeginInvoke((MethodInvoker)delegate { Close(); });
                }
            }
        }

        private void RecoverClicked(object sender, EventArgs e)
        {
            if (!File.Exists(ClientData.ProxySnapshotPath))
            {
                UpdateRecoveryState();
                return;
            }
            DialogResult answer = MessageBox.Show(
                "Клиент сохранит исходные настройки прокси и попытается остановить оставшийся процесс ядра. Продолжить?",
                "Восстановление сети", MessageBoxButtons.YesNo, MessageBoxIcon.Warning);
            if (answer != DialogResult.Yes) return;
            try
            {
                CoreRuntime.KillBundledCoreIfRunning();
                using (RegistryProxyProbe probe = new RegistryProxyProbe())
                {
                    if (probe.IsEnabledAt("127.0.0.1:1080"))
                    {
                        try { CoreRuntime.DisableSystemProxy(7000); } catch { }
                    }
                }
                ProxySnapshot snapshot = ProxySnapshot.Load(ClientData.ProxySnapshotPath);
                bool restored = snapshot.RestoreIfClientOwnsProxy();
                File.Delete(ClientData.ProxySnapshotPath);
                ClientData.DeleteSessionLink();
                AddLog(restored ? "Предыдущие настройки прокси возвращены." : "Настройки прокси не менялись: активный адрес принадлежит не клиенту.");
                MessageBox.Show(restored ? "Исходные настройки прокси восстановлены." :
                    "Настройки прокси не принадлежали этому клиенту. Изменений не внесено.",
                    "Готово", MessageBoxButtons.OK, MessageBoxIcon.Information);
            }
            catch (Exception ex)
            {
                AddLog("Восстановление не завершено: " + SafeMessage(ex.Message));
                MessageBox.Show("Не удалось завершить автоматическое восстановление.\r\n\r\n" + SafeMessage(ex.Message),
                    "Восстановление сети", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
            UpdateRecoveryState();
        }

        private void MonitorTick(object sender, EventArgs e)
        {
            if (coreProcess == null || stopping) return;
            try
            {
                if (coreProcess.HasExited)
                {
                    int code = coreProcess.ExitCode;
                    coreProcess.Dispose(); coreProcess = null;
                    AddLog("Ядро завершилось (код " + code + ").");
                    ClientData.DeleteSessionLink();
                    try
                    {
                        if (File.Exists(ClientData.ProxySnapshotPath))
                        {
                            ProxySnapshot snapshot = ProxySnapshot.Load(ClientData.ProxySnapshotPath);
                            if (snapshot.RestoreIfClientOwnsProxy()) AddLog("Исходные настройки прокси возвращены после завершения ядра.");
                            File.Delete(ClientData.ProxySnapshotPath);
                        }
                    }
                    catch (Exception ex) { AddLog("Проверьте восстановление прокси: " + SafeMessage(ex.Message)); }
                    connectButton.Text = "Подключить";
                    SetBusy(false, "Ядро остановилось");
                    UpdateRecoveryState();
                }
            }
            catch { }
        }

        private async void ClosingForm(object sender, FormClosingEventArgs e)
        {
            if (allowClose) return;
            if (starting)
            {
                e.Cancel = true;
                closeAfterStart = true;
                AddLog("Закрытие будет выполнено после завершения запуска и остановки ядра.");
                return;
            }
            if (stopping)
            {
                e.Cancel = true;
                closeAfterStop = true;
                return;
            }
            if (coreProcess != null)
            {
                e.Cancel = true;
                await StopCoreAsync();
                allowClose = true;
                Close();
            }
            else if (File.Exists(ClientData.ProxySnapshotPath))
            {
                e.Cancel = true;
                MessageBox.Show("Сначала восстановите настройки прокси через кнопку в окне клиента.",
                    "Нужно восстановление", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }

        private void CoreLog(string line)
        {
            if (String.IsNullOrWhiteSpace(line)) return;
            string lower = line.ToLowerInvariant();
            if (lower.Contains("listening") || lower.Contains("прокси слушает"))
                BeginInvoke((MethodInvoker)delegate { AddLog("Ядро подтвердило, что локальный прокси слушает порт."); });
            else if (lower.Contains("error") || lower.Contains("ошибка"))
                BeginInvoke((MethodInvoker)delegate { AddLog("Ядро сообщило об ошибке; чувствительная часть журнала скрыта."); });
        }

        private void SetBusy(bool busy, string state)
        {
            if (IsDisposed) return;
            connectButton.Enabled = !busy;
            UpdateConfigMode();
            systemProxyMode.Enabled = !busy && coreProcess == null;
            advancedMode.Enabled = !busy && coreProcess == null;
            stateLabel.Text = state;
            stateLabel.ForeColor = state.Contains("запущен") || state.Contains("Подключено")
                ? Color.FromArgb(85, 214, 190) : Color.FromArgb(174, 190, 198);
        }

        private void UpdateRecoveryState()
        {
            recoveryButton.Visible = File.Exists(ClientData.ProxySnapshotPath) && coreProcess == null;
            recoveryButton.Enabled = coreProcess == null;
        }

        private void AddLog(string text)
        {
            string safe = SafeMessage(text);
            activityLog.AppendText("[" + DateTime.Now.ToString("HH:mm:ss") + "]  " + safe + Environment.NewLine);
            activityLog.SelectionStart = activityLog.TextLength;
            activityLog.ScrollToCaret();
        }

        private static string SafeMessage(string text)
        {
            if (String.IsNullOrEmpty(text)) return String.Empty;
            int start = text.IndexOf("vless://", StringComparison.OrdinalIgnoreCase);
            return start < 0 ? text : text.Substring(0, start) + "[секрет скрыт]";
        }

        private static string ValidateLink(string link)
        {
            if (String.IsNullOrWhiteSpace(link)) return "Вставьте VLESS-ссылку.";
            if (link.IndexOfAny(new[] { '\r', '\n', '\0' }) >= 0) return "Ссылка должна занимать одну строку.";
            Uri uri;
            if (!Uri.TryCreate(link, UriKind.Absolute, out uri) || !String.Equals(uri.Scheme, "vless", StringComparison.OrdinalIgnoreCase))
                return "Ожидается ссылка формата vless://UUID@сервер:порт?...";
            if (String.IsNullOrWhiteSpace(uri.Host) || uri.Port < 1 || uri.Port > 65535 || String.IsNullOrWhiteSpace(uri.UserInfo))
                return "В ссылке должны быть указаны UUID, имя сервера и порт.";
            return null;
        }

        private void AddLogFromBackground(string text)
        {
            if (IsDisposed || !IsHandleCreated) return;
            BeginInvoke((MethodInvoker)delegate { AddLog(text); });
        }

        private void LinkInputKeyDown(object sender, KeyEventArgs e)
        {
            if (e.Control && e.KeyCode == Keys.V)
            {
                e.SuppressKeyPress = true;
                PasteLinkFromClipboard(sender, EventArgs.Empty);
            }
        }

        private void PasteLinkFromClipboard(object sender, EventArgs e)
        {
            try
            {
                if (!Clipboard.ContainsText())
                {
                    MessageBox.Show(this, "В буфере обмена нет текста со ссылкой.", "Вставка", MessageBoxButtons.OK, MessageBoxIcon.Information);
                    return;
                }
                string value = Clipboard.GetText(TextDataFormat.UnicodeText).Trim();
                if (value.IndexOfAny(new[] { '\r', '\n', '\0' }) >= 0)
                {
                    MessageBox.Show(this, "В буфере несколько строк. Скопируйте только одну ссылку VLESS.", "Вставка", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                    return;
                }
                linkInput.Focus();
                linkInput.SelectedText = value;
                AddLog("Ссылка вставлена из буфера обмена; она остаётся скрытой в поле.");
            }
            catch (Exception ex)
            {
                MessageBox.Show(this, "Не удалось прочитать буфер обмена. Попробуйте ещё раз.\r\n\r\n" + ex.Message,
                    "Вставка не удалась", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }

        private void EditAdvancedConfig(object sender, EventArgs e)
        {
            if (coreProcess != null || starting || stopping) return;
            using (ConfigStudio studio = new ConfigStudio())
                if (studio.ShowDialog(this) == DialogResult.OK)
                {
                    advancedMode.Checked = true;
                    AddLog("Выбран полный конфиг: " + Path.GetFileName(studio.SelectedConfigPath));
                }
        }

        private void UpdateConfigMode()
        {
            bool custom = advancedMode.Checked;
            profilePicker.Enabled = !custom && coreProcess == null && !starting && !stopping;
            nameInput.Enabled = !custom && coreProcess == null && !starting && !stopping;
            linkInput.Enabled = !custom && coreProcess == null && !starting && !stopping;
            configButton.Enabled = coreProcess == null && !starting && !stopping;
            if (endpointLabel != null) endpointLabel.Text = custom ? "Режим: полный конфиг ядра" : "Локальный прокси: 127.0.0.1:1080";
        }
    }
}
