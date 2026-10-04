$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$exe = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\dist\RealityClient.exe'))
$app = Start-Process -FilePath $exe -PassThru
try {
    $deadline = (Get-Date).AddSeconds(10)
    do { Start-Sleep -Milliseconds 150; $app.Refresh() }
    while ($app.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline)
    if ($app.MainWindowHandle -eq 0 -or $app.MainWindowTitle -ne 'Reality Client') {
        throw 'Окно клиента не появилось.'
    }
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
    $condition = [System.Windows.Automation.Condition]::TrueCondition
    $elements = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    $names = @($elements | ForEach-Object { $_.Current.Name })
    foreach ($required in @('Профиль подключения', 'Ссылка VLESS', 'Вставить', 'Сохранить профиль', 'Подключение', 'Подключить')) {
        if ($names -notcontains $required) { throw "В интерфейсе отсутствует элемент: $required" }
    }
    $window = $root.Current.BoundingRectangle
    foreach ($required in @('Название', 'Ссылка VLESS', 'Вставить')) {
        $matches = @($elements | Where-Object { $_.Current.Name -eq $required })
        if ($matches.Count -eq 0) { throw "Не найден элемент для проверки размеров: $required" }
        foreach ($element in $matches) {
            $bounds = $element.Current.BoundingRectangle
            if ($bounds.IsEmpty -or $bounds.Left -lt $window.Left -or $bounds.Right -gt $window.Right -or
                $bounds.Top -lt $window.Top -or $bounds.Bottom -gt $window.Bottom) {
                throw "Элемент '$required' частично находится за пределами окна."
            }
        }
    }
    Write-Output 'GUI_WINDOW_AND_CONTROLS=PASS'
}
finally {
    if (-not $app.HasExited) {
        $app.CloseMainWindow() | Out-Null
        $app.WaitForExit(7000) | Out-Null
    }
    if (-not $app.HasExited) { $app.Kill(); $app.WaitForExit(3000) | Out-Null }
}
