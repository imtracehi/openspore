; OpenSpore setup (Inno Setup 6).
;
; Per-user install without admin rights into %LOCALAPPDATA%\Programs\OpenSpore.
; OpenSpore ships no game files: the wizard links the player's own Spore
; install (detected from the registry and Steam libraries, or browsed to) and
; stores it in %APPDATA%\OpenSpore\settings.json, the file openspore.exe reads.
; The uninstaller removes only OpenSpore's program files; it asks before
; deleting the saves and settings in %APPDATA%\OpenSpore and never touches the
; Spore install.
;
; Build with installer\build-release.ps1 (it stages dist\OpenSpore-<version>\
; and calls ISCC when Inno Setup 6 is installed), or by hand:
;   ISCC.exe /DAppVersion=0.73.0 /DSourceDir=..\dist\OpenSpore-0.73.0 /DOutputDir=..\dist OpenSpore.iss
; Silent install with a given Spore folder:
;   OpenSpore-0.73.0-setup.exe /SILENT /SPOREPATH="D:\Games\Spore"
; Test install that leaves %APPDATA%\OpenSpore alone (settings go to /USERDIR):
;   OpenSpore-0.73.0-setup.exe /DIR="C:\Temp\os\prog" /USERDIR="C:\Temp\os\user"

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\OpenSpore-" + AppVersion
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

[Setup]
AppId={{6C1F7E52-3B8D-4E0A-9D5B-0F3A2C7B9E41}
AppName=OpenSpore
AppVersion={#AppVersion}
AppVerName=OpenSpore {#AppVersion}
AppPublisher=OpenSpore contributors
DefaultDirName={localappdata}\Programs\OpenSpore
DefaultGroupName=OpenSpore
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
LicenseFile={#SourceDir}\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename=OpenSpore-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
UninstallDisplayIcon={app}\openspore.exe
UninstallDisplayName=OpenSpore {#AppVersion}
VersionInfoVersion={#AppVersion}
VersionInfoDescription=OpenSpore setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\openspore.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README-player.txt"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{group}\OpenSpore"; Filename: "{app}\openspore.exe"; WorkingDir: "{app}"
Name: "{group}\OpenSpore - change Spore folder"; Filename: "{app}\openspore.exe"; Parameters: "--choose-spore"; WorkingDir: "{app}"
Name: "{group}\OpenSpore README"; Filename: "{app}\README-player.txt"
Name: "{group}\Uninstall OpenSpore"; Filename: "{uninstallexe}"
Name: "{autodesktop}\OpenSpore"; Filename: "{app}\openspore.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\openspore.exe"; Description: "{cm:LaunchProgram,OpenSpore}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

[Code]
var
  SporePage: TInputDirWizardPage;
  SporeRoot: String;

{ --- Spore install validation (same rules as src\spore_install.rs) --- }

function IsSporeRoot(const Dir: String): Boolean;
begin
  Result := (Dir <> '') and
    FileExists(AddBackslash(Dir) + 'Data\Spore_Game.package') and
    FileExists(AddBackslash(Dir) + 'Data\Spore_Graphics.package');
end;

function CleanDir(Dir: String): String;
begin
  Dir := Trim(RemoveQuotes(Trim(Dir)));
  StringChangeEx(Dir, '/', '\', True);
  Result := RemoveBackslashUnlessRoot(Dir);
end;

function IsRootChild(const Dir: String): Boolean;
var
  Leaf: String;
begin
  Leaf := Lowercase(ExtractFileName(Dir));
  Result := (Leaf = 'data') or (Leaf = 'dataep1') or (Leaf = 'sporebin') or
    (Leaf = 'sporebinep1') or (Leaf = 'bp1content') or (Leaf = 'bp1data');
end;

{ The install root for a chosen folder: the root itself, or the parent of its
  Data / Sporebin / SporebinEP1 / expansion data folder. '' when invalid. }
function SporeRootFor(Dir: String): String;
begin
  Result := '';
  Dir := CleanDir(Dir);
  if Dir = '' then
    exit;
  if IsSporeRoot(Dir) then
    Result := Dir
  else if IsRootChild(Dir) and IsSporeRoot(ExtractFileDir(Dir)) then
    Result := ExtractFileDir(Dir);
end;

{ What is missing, in the words openspore.exe uses. }
function MissingText(Dir: String): String;
var
  Root: String;
begin
  Dir := CleanDir(Dir);
  if not DirExists(Dir) then
  begin
    Result := 'This folder does not exist:' + #13#10 + Dir;
    exit;
  end;
  Root := Dir;
  if IsRootChild(Dir) then
    Root := ExtractFileDir(Dir);
  Result := '';
  if not FileExists(AddBackslash(Root) + 'Data\Spore_Game.package') then
    Result := 'Data\Spore_Game.package';
  if not FileExists(AddBackslash(Root) + 'Data\Spore_Graphics.package') then
  begin
    if Result <> '' then
      Result := Result + ' and ';
    Result := Result + 'Data\Spore_Graphics.package';
  end;
  Result := 'This folder is not a Spore install: ' + Result + ' not found in' + #13#10 + Root;
  if (not DirExists(AddBackslash(Root) + 'Data')) and
     (DirExists(AddBackslash(Root) + 'DataEP1') or DirExists(AddBackslash(Root) + 'SporebinEP1')) then
    Result := Result + #13#10#13#10 + 'This looks like a Galactic Adventures folder. Choose the main Spore folder instead.';
end;

{ --- Detection (same sources as src\spore_install.rs) --- }

function LibraryFromSteamPath(const Dir: String): String;
var
  P: Integer;
begin
  { <library>\steamapps\common\... -> <library> }
  Result := '';
  P := Pos('\steamapps\common\', Lowercase(Dir) + '\');
  if P > 1 then
    Result := Copy(Dir, 1, P - 1);
end;

procedure AddUnique(var List: TArrayOfString; const Value: String);
var
  I: Integer;
  V: String;
begin
  V := CleanDir(Value);
  if V = '' then
    exit;
  for I := 0 to GetArrayLength(List) - 1 do
    if CompareText(List[I], V) = 0 then
      exit;
  SetArrayLength(List, GetArrayLength(List) + 1);
  List[GetArrayLength(List) - 1] := V;
end;

{ The value of a "key" "value" line of a Valve KeyValues file, unescaped. }
function VdfValue(const Line, Key: String): String;
var
  S: String;
  P: Integer;
begin
  Result := '';
  S := Trim(Line);
  if Pos('"' + Lowercase(Key) + '"', Lowercase(S)) <> 1 then
    exit;
  Delete(S, 1, Length(Key) + 2);
  P := Pos('"', S);
  if P = 0 then
    exit;
  Delete(S, 1, P);
  P := Pos('"', S);
  if P = 0 then
    exit;
  Result := Copy(S, 1, P - 1);
  StringChangeEx(Result, '\\', '\', True);
end;

procedure AddSteamLibrary(var Libraries: TArrayOfString; const SteamDir: String);
var
  Lines: TArrayOfString;
  I: Integer;
  V: String;
begin
  if SteamDir = '' then
    exit;
  AddUnique(Libraries, SteamDir);
  if LoadStringsFromFile(AddBackslash(CleanDir(SteamDir)) + 'steamapps\libraryfolders.vdf', Lines) then
    for I := 0 to GetArrayLength(Lines) - 1 do
    begin
      V := VdfValue(Lines[I], 'path');
      if V <> '' then
        AddUnique(Libraries, V);
    end;
end;

{ <library>\steamapps\common\<installdir of app 17390>, else ...\common\Spore. }
function SteamSporeIn(const Lib: String): String;
var
  Lines: TArrayOfString;
  I: Integer;
  V: String;
begin
  Result := '';
  if LoadStringsFromFile(AddBackslash(Lib) + 'steamapps\appmanifest_17390.acf', Lines) then
    for I := 0 to GetArrayLength(Lines) - 1 do
    begin
      V := VdfValue(Lines[I], 'installdir');
      if (V <> '') and (Pos('\', V) = 0) and (Pos('/', V) = 0) and (Pos(':', V) = 0) and (V <> '..') then
      begin
        Result := SporeRootFor(AddBackslash(Lib) + 'steamapps\common\' + V);
        if Result <> '' then
          exit;
      end;
    end;
  Result := SporeRootFor(AddBackslash(Lib) + 'steamapps\common\Spore');
end;

procedure AddRegistryValues(var Dirs: TArrayOfString; const RootKey: Integer; const SubKey: String);
var
  V: String;
begin
  if RegQueryStringValue(RootKey, SubKey, 'InstallLoc', V) then AddUnique(Dirs, V);
  if RegQueryStringValue(RootKey, SubKey, 'Install Dir', V) then AddUnique(Dirs, V);
  if RegQueryStringValue(RootKey, SubKey, 'DataDir', V) then AddUnique(Dirs, V);
end;

procedure AddEaKeys(var Dirs: TArrayOfString; const RootKey: Integer);
var
  Names: TArrayOfString;
  I: Integer;
begin
  { Electronic Arts\SPORE, SPORE_EP1, SPORE(TM) Creepy & Cute Parts Pack, ... }
  if RegGetSubkeyNames(RootKey, 'SOFTWARE\Electronic Arts', Names) then
    for I := 0 to GetArrayLength(Names) - 1 do
      if Pos('spore', Lowercase(Names[I])) = 1 then
        AddRegistryValues(Dirs, RootKey, 'SOFTWARE\Electronic Arts\' + Names[I]);
end;

procedure AddGogKeys(var Dirs: TArrayOfString; const RootKey: Integer);
var
  Names: TArrayOfString;
  I: Integer;
  GameName, V: String;
begin
  if RegGetSubkeyNames(RootKey, 'SOFTWARE\GOG.com\Games', Names) then
    for I := 0 to GetArrayLength(Names) - 1 do
      if RegQueryStringValue(RootKey, 'SOFTWARE\GOG.com\Games\' + Names[I], 'gameName', GameName) and
         (Pos('spore', Lowercase(GameName)) > 0) and
         RegQueryStringValue(RootKey, 'SOFTWARE\GOG.com\Games\' + Names[I], 'path', V) then
        AddUnique(Dirs, V);
end;

{ The "spore_path" string of a settings.json, unescaped ('' if none). }
function SettingsSporePath(const FileName: String): String;
var
  Lines: TArrayOfString;
  Content, S: String;
  I, P: Integer;
begin
  Result := '';
  if not LoadStringsFromFile(FileName, Lines) then
    exit;
  Content := '';
  for I := 0 to GetArrayLength(Lines) - 1 do
    Content := Content + Lines[I] + ' ';
  P := Pos('"spore_path"', Content);
  if P = 0 then
    exit;
  S := Copy(Content, P + 12, Length(Content));
  P := Pos('"', S);
  if P = 0 then
    exit;
  Delete(S, 1, P);
  { Read up to the closing quote, honouring backslash escapes. }
  I := 1;
  while I <= Length(S) do
  begin
    if S[I] = '\' then
    begin
      if I < Length(S) then
        Result := Result + S[I + 1];
      I := I + 2;
    end
    else if S[I] = '"' then
      exit
    else
    begin
      Result := Result + S[I];
      I := I + 1;
    end;
  end;
  Result := '';
end;

{ %APPDATA%\OpenSpore, or /USERDIR=<folder> (tests: the same folder as
  openspore.exe's OPENSPORE_USER_DIR). }
function UserDataDir: String;
begin
  Result := CleanDir(ExpandConstant('{param:USERDIR|}'));
  if Result = '' then
    Result := ExpandConstant('{userappdata}\OpenSpore');
end;

function SettingsFile: String;
begin
  Result := AddBackslash(UserDataDir) + 'settings.json';
end;

function DetectSpore: String;
var
  RegistryDirs, Libraries: TArrayOfString;
  I: Integer;
  V: String;
begin
  { 1. A folder linked earlier (upgrade / reinstall). }
  Result := SporeRootFor(SettingsSporePath(SettingsFile));
  if Result <> '' then
    exit;
  { 2. Registry entries of non-Steam installs (EA / Origin / disc, GOG). }
  SetArrayLength(RegistryDirs, 0);
  AddEaKeys(RegistryDirs, HKLM32);
  AddGogKeys(RegistryDirs, HKLM32);
  if IsWin64 then
  begin
    AddEaKeys(RegistryDirs, HKLM64);
    AddGogKeys(RegistryDirs, HKLM64);
  end;
  for I := 0 to GetArrayLength(RegistryDirs) - 1 do
    if LibraryFromSteamPath(RegistryDirs[I]) = '' then
    begin
      Result := SporeRootFor(RegistryDirs[I]);
      if Result <> '' then
        exit;
    end;
  { 3. Steam libraries (the base app folder, not the Galactic Adventures
       folder Steam's EA keys point at). }
  SetArrayLength(Libraries, 0);
  if RegQueryStringValue(HKCU, 'Software\Valve\Steam', 'SteamPath', V) then
    AddSteamLibrary(Libraries, V);
  if RegQueryStringValue(HKLM32, 'SOFTWARE\Valve\Steam', 'InstallPath', V) then
    AddSteamLibrary(Libraries, V);
  AddSteamLibrary(Libraries, ExpandConstant('{commonpf32}\Steam'));
  for I := 0 to GetArrayLength(RegistryDirs) - 1 do
    AddSteamLibrary(Libraries, LibraryFromSteamPath(RegistryDirs[I]));
  for I := 0 to GetArrayLength(Libraries) - 1 do
  begin
    Result := SteamSporeIn(Libraries[I]);
    if Result <> '' then
      exit;
  end;
  if IsWin64 and RegQueryStringValue(HKLM64, 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Steam App 17390', 'InstallLocation', V) then
  begin
    Result := SporeRootFor(V);
    if Result <> '' then
      exit;
  end;
  { 4. Registry entries inside a Steam library, as a last resort. }
  for I := 0 to GetArrayLength(RegistryDirs) - 1 do
  begin
    Result := SporeRootFor(RegistryDirs[I]);
    if Result <> '' then
      exit;
  end;
  Result := '';
end;

{ --- settings.json (keeps other keys when the file exists) --- }

function JsonEscape(const S: String): String;
begin
  Result := S;
  StringChangeEx(Result, '\', '\\', True);
  StringChangeEx(Result, '"', '\"', True);
end;

function WriteSettings(const Root: String): Boolean;
var
  Lines, OutLines: TArrayOfString;
  Content, Value: String;
  I, P, Q: Integer;
begin
  Value := '"' + JsonEscape(Root) + '"';
  Content := '';
  if LoadStringsFromFile(SettingsFile, Lines) then
    for I := 0 to GetArrayLength(Lines) - 1 do
      Content := Content + Lines[I] + #13#10;
  P := Pos('"spore_path"', Content);
  if P > 0 then
  begin
    { Replace the old string value. }
    Q := P + 12;
    while (Q <= Length(Content)) and (Content[Q] <> '"') do
      Q := Q + 1;
    P := Q;
    Q := Q + 1;
    while (Q <= Length(Content)) and (Content[Q] <> '"') do
    begin
      if Content[Q] = '\' then
        Q := Q + 1;
      Q := Q + 1;
    end;
    if Q <= Length(Content) then
      Content := Copy(Content, 1, P - 1) + Value + Copy(Content, Q + 1, Length(Content))
    else
      Content := '';
  end
  else if Pos('{', Content) > 0 then
  begin
    P := Pos('{', Content);
    if Pos('"', Copy(Content, P, Length(Content))) > 0 then
      Insert(#13#10 + '  "spore_path": ' + Value + ',', Content, P + 1)
    else
      Content := '';
  end
  else
    Content := '';
  if Content = '' then
    Content := '{' + #13#10 + '  "spore_path": ' + Value + #13#10 + '}' + #13#10;
  ForceDirectories(ExtractFileDir(SettingsFile));
  SetArrayLength(OutLines, 1);
  OutLines[0] := Content;
  Result := SaveStringsToUTF8File(SettingsFile, OutLines, False);
end;

{ --- Wizard --- }

procedure InitializeWizard;
var
  Given: String;
begin
  SporePage := CreateInputDirPage(wpSelectDir,
    'Your Spore installation',
    'OpenSpore uses the game files of your own copy of Spore.',
    'OpenSpore includes no game files. Choose the folder where Spore is installed ' +
    '(Steam, EA app / Origin, GOG or disc): the folder that contains the Data folder, ' +
    'for example ...\steamapps\common\Spore or C:\Program Files (x86)\Electronic Arts\SPORE.' + #13#10#13#10 +
    'OpenSpore only reads this folder and never changes it. You can change it later with ' +
    'the Start Menu entry "OpenSpore - change Spore folder".',
    False, '');
  SporePage.Add('Spore folder:');
  Given := ExpandConstant('{param:SPOREPATH|}');
  if Given <> '' then
    SporePage.Values[0] := Given
  else
    SporePage.Values[0] := DetectSpore;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = SporePage.ID then
  begin
    SporeRoot := SporeRootFor(SporePage.Values[0]);
    if SporeRoot <> '' then
      SporePage.Values[0] := SporeRoot
    else if CleanDir(SporePage.Values[0]) = '' then
      Result := SuppressibleMsgBox('No Spore folder was chosen. Continue anyway?' + #13#10#13#10 +
        'OpenSpore will ask for your Spore folder when it starts.',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDYES) = IDYES
    else
    begin
      SuppressibleMsgBox(MissingText(SporePage.Values[0]) + #13#10#13#10 +
        'Choose the Spore folder whose Data folder holds Spore_Game.package and Spore_Graphics.package.',
        mbError, MB_OK, IDOK);
      Result := False;
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    { Silent installs skip the page: validate whatever it holds. }
    if SporeRoot = '' then
      SporeRoot := SporeRootFor(SporePage.Values[0]);
    if SporeRoot <> '' then
    begin
      if not WriteSettings(SporeRoot) then
        SuppressibleMsgBox('Could not write ' + SettingsFile + '. OpenSpore will ask for your Spore folder when it starts.',
          mbInformation, MB_OK, IDOK);
    end
    else
      Log('No valid Spore folder chosen; OpenSpore will ask on first start.');
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  UserDir, CacheDir: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    { Runtime files (the icon extracted from the player's install, caches). }
    CacheDir := ExpandConstant('{localappdata}\OpenSpore');
    if DirExists(CacheDir) then
      DelTree(CacheDir, True, True, True);
    { Saves and settings: only when the player says so. }
    UserDir := UserDataDir;
    if DirExists(UserDir) and (not UninstallSilent) then
      if MsgBox('Also delete your OpenSpore saves and settings?' + #13#10#13#10 + UserDir + #13#10#13#10 +
        'Choose No to keep them for a later install. Your Spore installation is not touched either way.',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(UserDir, True, True, True);
  end;
end;
