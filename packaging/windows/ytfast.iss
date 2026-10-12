; YTFast's Windows installer, made with Inno Setup 6 from a release build
; (CI makes it on every build, .github/workflows/ci.yml):
;
;   iscc /DVersion=1.3.0 /DBinary=...\ytfast.exe ^
;        /DNotices=...\THIRD-PARTY-NOTICES.txt /DOutputDir=dist ^
;        packaging\windows\ytfast.iss
;
; It needs no administrator: YTFast goes to the user's own Programs folder
; (%LOCALAPPDATA%\Programs\YTFast), with an entry in the Start menu, a
; shortcut on the desktop (a tick box, ticked), and an entry in Settings >
; Apps that uninstalls it. ytfast-installer.txt beside the program tells
; fastframe-update that this copy updates by running the next release's
; installer silently (`ytfast-v<version>-x86_64-pc-windows-msvc-setup.exe`).
; The settings, the sign-in and the downloaded helpers are not in that
; folder: installing and uninstalling leave them be.
;
; Adapted from Spotifast's (packaging/windows/spotifast.iss, MIT, Copyright
; (c) 2026 Carmine Paolino).

#ifndef Version
  #error Version must be defined on the ISCC command line
#endif
#ifndef Binary
  #error Binary must be defined on the ISCC command line
#endif
#ifndef Notices
  #error Notices must be defined on the ISCC command line
#endif
#ifndef OutputDir
  #error OutputDir must be defined on the ISCC command line
#endif

#define AppName "YTFast"
#define AppExeName "YTFast.exe"

[Setup]
; Never change: this is how Windows tells an update from a new program.
AppId={{E5C965A4-1C85-441A-8585-6AE2BA46004E}
AppName={#AppName}
AppVersion={#Version}
AppVerName={#AppName} {#Version}
AppPublisher=Likheet Shetty
AppPublisherURL=https://github.com/Likheet/YTFast
AppSupportURL=https://github.com/Likheet/YTFast/issues
AppUpdatesURL=https://github.com/Likheet/YTFast/releases/latest
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Always the user's own Programs folder, where updates install too.
DisableDirPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=ytfast-v{#Version}-x86_64-pc-windows-msvc-setup
SetupIconFile=..\icons\ytfast.ico
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
; The file version has to be numbers: a release candidate's -rc1 comes off.
#define Dash Pos("-", Version)
#if Dash > 0
  #define NumericVersion Copy(Version, 1, Dash - 1)
#else
  #define NumericVersion Version
#endif
VersionInfoVersion={#NumericVersion}.0

[Tasks]
Name: "desktopicon"; Description: "Put a shortcut on the desktop"; GroupDescription: "Shortcuts:"

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "{#AppExeName}"; Flags: ignoreversion
Source: "{#Notices}"; DestDir: "{app}"; DestName: "THIRD-PARTY-NOTICES.txt"; Flags: ignoreversion
Source: "ytfast-installer.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Registry]
; Settings' "Start with the computer" adds YTFast to the user's Run list
; (crates/ytfast/src/login.rs); uninstalling takes it away. Nothing is added
; here.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "YTFast"; Flags: uninsdeletevalue dontcreatekey

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Open YTFast"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; What YTFast made beside itself: its updates' folders.
Type: filesandordirs; Name: "{app}"

[Code]
// The YTFast programs running: the one installed here (Exe), or any copy
// (Exe empty, a copy opened from Downloads say). Close ends them at once.
function YTFastRunning(Exe: String; Close: Boolean): Boolean;
var
  Locator, Services, Processes, Process, Path: Variant;
  Index: Integer;
begin
  Result := False;
  try
    Locator := CreateOleObject('WbemScripting.SWbemLocator');
    Services := Locator.ConnectServer('', 'root\CIMV2');
    Processes := Services.ExecQuery('SELECT * FROM Win32_Process WHERE Name = ''{#AppExeName}''');
    for Index := 0 to Processes.Count - 1 do begin
      Process := Processes.ItemIndex(Index);
      Path := Process.ExecutablePath;
      // Another program can have the same name: by name alone only when
      // any copy of YTFast is meant.
      if (Exe = '') or ((not VarIsNull(Path)) and (CompareText(Path, Exe) = 0)) then begin
        Result := True;
        if Close then
          Log('Closing YTFast: result ' + IntToStr(Process.Terminate(0)));
      end;
    end;
  except
    Log('Could not look for YTFast running: ' + GetExceptionMessage);
  end;
end;

// One YTFast runs at a time: a copy running (from Downloads, say, or
// playing on with its window closed) would keep the one installed from
// opening, so it is closed first, when the person agrees. An update runs
// silently, after fastframe-update has closed YTFast.
function InitializeSetup(): Boolean;
begin
  Result := True;
  if WizardSilent then
    Exit;
  while YTFastRunning('', False) do begin
    if TaskDialogMsgBox('YTFast is running.',
      'Close it to install YTFast? The music stops; your queue is kept.',
      mbConfirmation, MB_OKCANCEL, ['Close YTFast'], 0) <> IDOK then begin
      Result := False;
      Exit;
    end;
    YTFastRunning('', True);
    Sleep(500);
  end;
end;

// CloseApplications covers Setup, not Uninstall. Check the installed file
// before removing anything, including the uninstaller needed to try again.
function InitializeUninstall: Boolean;
var
  Exe: String;
  Probe: TFileStream;
begin
  Result := False;
  Exe := ExpandConstant('{app}\{#AppExeName}');
  while FileExists(Exe) do begin
    try
      // Windows denies write access to a running program. Nothing is
      // written: opening it also finds other locks on this copy.
      Probe := TFileStream.Create(Exe, fmOpenReadWrite or fmShareExclusive);
      Probe.Free;
      Result := True;
      Exit;
    except
      Log('Cannot open the installed program for uninstall: ' + GetExceptionMessage);
    end;
    if UninstallSilent then
      Exit;
    if TaskDialogMsgBox('YTFast is running.',
      'Close it to uninstall YTFast? The music stops.',
      mbConfirmation, MB_OKCANCEL, ['Close YTFast'], 0) <> IDOK then
      Exit;
    YTFastRunning(Exe, True);
    Sleep(500);
  end;
  Result := True;
end;
