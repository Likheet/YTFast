# Starts YTFast's demo at YouTube Music's measuring size, presses its
# buttons by their screen-reader names, and saves pictures of it, without
# moving the mouse or taking the keyboard. Windows only.
#
#   .\tools\look\demo.ps1 start                 # target\release (else debug) ytfast.exe --demo
#   .\tools\look\demo.ps1 start -Exe C:\path\YTFast.exe
#   .\tools\look\demo.ps1 press -Names "Explore"          # ";" between several; "Name#2" = the second
#   .\tools\look\demo.ps1 search -Text "mara"             # types in the search box and searches
#   .\tools\look\demo.ps1 key -Key Escape
#   .\tools\look\demo.ps1 rclick -X 400 -Y 300             # a right-click there, in points
#   .\tools\look\demo.ps1 shot -Name explore              # saves target\look\app\explore.png
#   .\tools\look\demo.ps1 list                            # every named thing on screen, in points
#   .\tools\look\demo.ps1 size -Width 960 -Height 600     # the inside of the window, in points
#   .\tools\look\demo.ps1 tour                            # pictures of every main screen
#   .\tools\look\demo.ps1 cost -Seconds 10                # CPU and memory while left alone
#   .\tools\look\demo.ps1 stop                            # closes the demo it started (never another)
#
# Pictures are of the window's inside at the screen's own pixels (1920x1230
# for 1280x820 points at 150% scaling). They go to target\look\app, which
# git ignores.
param(
    [Parameter(Mandatory, Position = 0)]
    [ValidateSet('start', 'press', 'search', 'key', 'rclick', 'shot', 'list', 'size', 'tour', 'cost', 'stop')]
    [string]$Action,
    [string]$Exe,
    [string]$Names,
    [string]$Text,
    [ValidateSet('Return', 'Escape', 'Right', 'Left', 'Up', 'Down', 'Space', 'Tab')][string]$Key,
    [string]$Name,
    [double]$Width = 1280,
    [double]$Height = 820,
    [int]$PauseMs = 600,
    [int]$Seconds = 10,
    [double]$X,
    [double]$Y,
    [switch]$NoEnter
)

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$shots = Join-Path $root 'target\look\app'

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
if (-not ('LookWin2' -as [type])) {
    Add-Type -ReferencedAssemblies System.Drawing @'
using System;
using System.Runtime.InteropServices;
public static class LookWin2 {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hgt, uint flags);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern short VkKeyScan(char c);
    [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint code, uint type);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
'@
}
[void][LookWin2]::SetProcessDPIAware()

# The demo this tool started: its process number is kept in
# target\look\demo.pid, so the tool never acts on another demo (another
# assistant may have one open) or on a real YTFast.
$pidFile = Join-Path $root 'target\look\demo.pid'
function Get-Demo {
    if (-not (Test-Path $pidFile)) { throw 'no demo started by this tool (start it with: demo.ps1 start)' }
    $id = [int](Get-Content $pidFile -Raw)
    $process = Get-Process -Id $id -ErrorAction SilentlyContinue
    $line = (Get-CimInstance Win32_Process -Filter "ProcessId = $id" -ErrorAction SilentlyContinue).CommandLine
    if (-not $process -or $line -notmatch '--demo') { throw "the demo this tool started (process $id) is no longer running" }
    if ($process.MainWindowHandle -eq [IntPtr]::Zero) { throw 'the demo has no window yet' }
    $process
}

function Get-Scale([IntPtr]$hwnd) { [LookWin2]::GetDpiForWindow($hwnd) / 96.0 }

function Set-Size([System.Diagnostics.Process]$demo, [double]$w, [double]$h) {
    $hwnd = $demo.MainWindowHandle
    $scale = Get-Scale $hwnd
    [void][LookWin2]::ShowWindow($hwnd, 9) # restore, if maximised or minimised
    Start-Sleep -Milliseconds 300
    $outer = New-Object LookWin2+RECT; $inner = New-Object LookWin2+RECT
    [void][LookWin2]::GetWindowRect($hwnd, [ref]$outer); [void][LookWin2]::GetClientRect($hwnd, [ref]$inner)
    $frameW = ($outer.R - $outer.L) - ($inner.R - $inner.L)
    $frameH = ($outer.B - $outer.T) - ($inner.B - $inner.T)
    $flags = 0x0002 -bor 0x0004 -bor 0x0010 # keep its place and order, do not activate
    # Twice: the first change can change the frame (after a maximised window).
    for ($i = 0; $i -lt 2; $i++) {
        [void][LookWin2]::GetWindowRect($hwnd, [ref]$outer); [void][LookWin2]::GetClientRect($hwnd, [ref]$inner)
        $frameW = ($outer.R - $outer.L) - ($inner.R - $inner.L)
        $frameH = ($outer.B - $outer.T) - ($inner.B - $inner.T)
        [void][LookWin2]::SetWindowPos($hwnd, [IntPtr]::Zero, 0, 0, [int][math]::Round($w * $scale) + $frameW, [int][math]::Round($h * $scale) + $frameH, $flags)
        Start-Sleep -Milliseconds 400
    }
    [void][LookWin2]::GetClientRect($hwnd, [ref]$inner)
    "inside: {0}x{1} points ({2}x{3} pixels at {4:P0})" -f (($inner.R - $inner.L) / $scale), (($inner.B - $inner.T) / $scale), ($inner.R - $inner.L), ($inner.B - $inner.T), $scale
}

function Get-Named([System.Diagnostics.Process]$demo) {
    $top = [System.Windows.Automation.AutomationElement]::FromHandle($demo.MainWindowHandle)
    $top.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
}

function Invoke-Named([System.Diagnostics.Process]$demo, [string]$steps) {
    foreach ($step in ($steps.Split(';') | ForEach-Object { $_.Trim() } | Where-Object { $_ })) {
        $want = $step; $nth = 1
        if ($step -match '^(.*)#(\d+)$') { $want = $Matches[1]; $nth = [int]$Matches[2] }
        $found = @(Get-Named $demo | Where-Object { $_.Current.Name -eq $want })
        if ($found.Count -lt $nth) { "not found: $step ($($found.Count) named so)"; continue }
        $element = $found[$nth - 1]
        $pattern = $null
        if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { $pattern.Invoke(); "pressed: $step" }
        elseif ($element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) { $pattern.Toggle(); "toggled: $step" }
        elseif ($element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { $pattern.Select(); "selected: $step" }
        else { "cannot press: $step" }
        Start-Sleep -Milliseconds $PauseMs
    }
}

# A key pressed and let go, posted to the window (it need not have the
# keyboard).
function Send-Key([System.Diagnostics.Process]$demo, [uint32]$vk) {
    $hwnd = $demo.MainWindowHandle
    $scan = [LookWin2]::MapVirtualKey($vk, 0)
    $down = 1 -bor ($scan -shl 16)
    $up = [int64]$down -bor 0xC0000000
    [void][LookWin2]::PostMessage($hwnd, 0x0100, [IntPtr]$vk, [IntPtr]$down)
    Start-Sleep -Milliseconds 30
    [void][LookWin2]::PostMessage($hwnd, 0x0101, [IntPtr]$vk, [IntPtr]$up)
    Start-Sleep -Milliseconds 30
}

$keys = @{ Return = 0x0D; Escape = 0x1B; Right = 0x27; Left = 0x25; Up = 0x26; Down = 0x28; Space = 0x20; Tab = 0x09 }

function Save-Shot([System.Diagnostics.Process]$demo, [string]$name) {
    New-Item -ItemType Directory -Force $shots | Out-Null
    Start-Sleep -Milliseconds $PauseMs
    $hwnd = $demo.MainWindowHandle
    $inner = New-Object LookWin2+RECT
    [void][LookWin2]::GetClientRect($hwnd, [ref]$inner)
    $w = $inner.R - $inner.L; $h = $inner.B - $inner.T
    if ($w -le 0 -or $h -le 0) { throw 'the window has no size (minimised?)' }
    $bitmap = New-Object System.Drawing.Bitmap $w, $h
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $hdc = $graphics.GetHdc()
    [void][LookWin2]::PrintWindow($hwnd, $hdc, 3) # the inside only, drawn by the window itself
    $graphics.ReleaseHdc($hdc); $graphics.Dispose()
    $out = Join-Path $shots "$name.png"
    $bitmap.Save($out, [System.Drawing.Imaging.ImageFormat]::Png); $bitmap.Dispose()
    "saved $out (${w}x${h})"
}

function Search-For([System.Diagnostics.Process]$demo, [string]$words) {
    $box = Get-Named $demo | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit } | Select-Object -First 1
    if (-not $box) { throw 'no search box' }
    $box.SetFocus(); Start-Sleep -Milliseconds 300
    # The app ignores a screen reader's "set the value", so the words are
    # typed: each letter a key posted to the window.
    foreach ($c in $words.ToCharArray()) { Send-Key $demo ([uint32]([LookWin2]::VkKeyScan($c) -band 0xFF)) }
    Start-Sleep -Milliseconds $PauseMs
    if ($NoEnter) { return "typed: $words" }
    Send-Key $demo $keys.Return
    "searched: $words"
}

switch ($Action) {
    'start' {
        if (-not $Exe) {
            $Exe = @('target\release\ytfast.exe', 'target\debug\ytfast.exe') | ForEach-Object { Join-Path $root $_ } | Where-Object { Test-Path $_ } | Select-Object -First 1
            if (-not $Exe) { throw 'no ytfast.exe built in target\ (build it, or pass -Exe)' }
        }
        $process = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
        New-Item -ItemType Directory -Force (Split-Path $pidFile) | Out-Null
        Set-Content -Path $pidFile -Value $process.Id
        for ($i = 0; $i -lt 50 -and $process.MainWindowHandle -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 200; $process.Refresh() }
        Start-Sleep -Milliseconds 1200
        "started the demo (process $($process.Id)) from $Exe"
        Set-Size $process $Width $Height
    }
    'size' { Set-Size (Get-Demo) $Width $Height }
    'press' { Invoke-Named (Get-Demo) $Names }
    'search' { Search-For (Get-Demo) $Text }
    'key' { Send-Key (Get-Demo) $keys[$Key]; "pressed key $Key" }
    'rclick' {
        # The pointer is posted there, then the right button pressed and
        # let go (the real pointer does not move).
        $demo = Get-Demo
        $hwnd = $demo.MainWindowHandle
        $scale = Get-Scale $hwnd
        $at = [IntPtr]((([int]($Y * $scale)) -shl 16) -bor (([int]($X * $scale)) -band 0xFFFF))
        [void][LookWin2]::PostMessage($hwnd, 0x0200, [IntPtr]0, $at)
        [void][LookWin2]::PostMessage($hwnd, 0x0204, [IntPtr]2, $at)
        [void][LookWin2]::PostMessage($hwnd, 0x0205, [IntPtr]0, $at)
        "right-clicked at $X,$Y"
    }
    'shot' { Save-Shot (Get-Demo) $Name }
    'list' {
        $demo = Get-Demo
        $scale = Get-Scale $demo.MainWindowHandle
        $origin = New-Object LookWin2+POINT; [void][LookWin2]::ClientToScreen($demo.MainWindowHandle, [ref]$origin)
        foreach ($e in Get-Named $demo) {
            $c = $e.Current; $r = $c.BoundingRectangle
            if ($r.IsEmpty -or -not $c.Name) { continue }
            "{0,-12} {1,-40} {2,7:0.0},{3,7:0.0} {4,6:0.0}x{5,-6:0.0}" -f $c.LocalizedControlType, $c.Name, (($r.X - $origin.X) / $scale), (($r.Y - $origin.Y) / $scale), ($r.Width / $scale), ($r.Height / $scale)
        }
    }
    'tour' {
        # Every main screen, in the demo's made-up music. Names are the
        # buttons' screen-reader names (see AGENTS.md).
        $demo = Get-Demo
        Invoke-Named $demo 'Home'; Save-Shot $demo 'home'
        Invoke-Named $demo 'Explore'; Save-Shot $demo 'explore'
        Invoke-Named $demo 'Library'; Save-Shot $demo 'library'
        Invoke-Named $demo 'Songs'; Save-Shot $demo 'library-songs'
        Invoke-Named $demo 'Playlists'
        Invoke-Named $demo 'Road trip'; Save-Shot $demo 'playlist'
        Invoke-Named $demo 'Liked Music'; Save-Shot $demo 'liked'
        Search-For $demo 'mara'; Save-Shot $demo 'search'
        Invoke-Named $demo 'Home'
        Invoke-Named $demo 'Postcards'; Save-Shot $demo 'album'
        Invoke-Named $demo 'Home'
        Invoke-Named $demo 'Mara Sol'; Save-Shot $demo 'artist'
        Invoke-Named $demo 'Shuffle'; Start-Sleep -Milliseconds 800; Save-Shot $demo 'player-bar'
        Invoke-Named $demo 'Open the player page'; Save-Shot $demo 'player-upnext'
        Invoke-Named $demo 'LYRICS'; Save-Shot $demo 'player-lyrics'
        Invoke-Named $demo 'RELATED'; Save-Shot $demo 'player-related'
        Invoke-Named $demo 'Close the player page'
        Invoke-Named $demo 'Menu'; Save-Shot $demo 'mini-guide'
        Invoke-Named $demo 'Menu'
    }
    'cost' {
        # What the window costs while nobody touches it: the share of one
        # CPU core it used over the last few seconds, and its memory. Idle
        # (nothing playing) must stay at 0%; playing, it wakes about four
        # times a second.
        $demo = Get-Demo
        $before = $demo.TotalProcessorTime
        Start-Sleep -Seconds $Seconds
        $demo.Refresh()
        $used = ($demo.TotalProcessorTime - $before).TotalSeconds
        "CPU: {0:0.00}% of one core over {1} s" -f (100 * $used / $Seconds), $Seconds
        "memory: {0:0} MB private, {1:0} MB working set" -f ($demo.PrivateMemorySize64 / 1MB), ($demo.WorkingSet64 / 1MB)
    }
    'stop' {
        $demo = Get-Demo
        [void]$demo.CloseMainWindow()
        if (-not $demo.WaitForExit(5000)) { $demo.Kill() }
        Remove-Item $pidFile -ErrorAction SilentlyContinue
        "closed the demo (process $($demo.Id))"
    }
}
