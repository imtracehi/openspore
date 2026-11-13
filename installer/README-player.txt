OpenSpore
=========

OpenSpore is an open-source re-creation of the game engine of Spore (2008).
It is a work in progress: expect missing features, differences from the
original game and bugs. It is not made or endorsed by Maxis or Electronic Arts.

OpenSpore contains NO game files. It plays with the game files of YOUR OWN
installed copy of Spore, which it only reads and never changes.


Requirements
------------
- Windows 10 or 11, 64-bit, with a graphics card that supports Vulkan.
- An installed copy of Spore: Steam, EA app / Origin, GOG or the original disc.
  Galactic Adventures and the parts packs are used when they are installed.


Install
-------
Setup (recommended): run OpenSpore-<version>-setup.exe. It needs no
administrator rights and installs to %LOCALAPPDATA%\Programs\OpenSpore. On the
page "Your Spore installation" it shows the Spore folder it found; check it
(or click Browse) and continue.

Portable zip: extract OpenSpore-<version>-portable.zip anywhere and run
openspore.exe. The file portable.txt next to the exe keeps your saves and
settings in that folder. Delete portable.txt to keep them in your user folder
(see below) instead.

The Spore folder is the one that contains the "Data" folder, for example:
  ...\Steam\steamapps\common\Spore
  C:\Program Files (x86)\Electronic Arts\SPORE
You may also pick its Data or SporebinEP1 folder.


First start / changing the Spore folder
---------------------------------------
If OpenSpore cannot find Spore, it asks for the folder when it starts. To
choose another folder later, use the Start Menu entry
"OpenSpore - change Spore folder", or run:
  openspore.exe --choose-spore
You can also start it with a folder given directly:
  openspore.exe --spore-path "D:\Games\Spore"


Where OpenSpore keeps your files
--------------------------------
  Settings (the linked Spore folder):  %APPDATA%\OpenSpore\settings.json
  Saves:                               %APPDATA%\OpenSpore\saves\
  Cached files:                        %LOCALAPPDATA%\OpenSpore\
In portable mode all of these stay next to openspore.exe (cached files in its
"cache" folder). A save left next to openspore.exe by an older build keeps
being used where it is.

Uninstalling (Settings > Apps, or the Start Menu entry) removes OpenSpore's
program files and cached files. It asks before deleting your saves and
settings. It never touches your Spore installation.


Feedback
--------
Please report bugs, crashes and differences from the original game on the
project's GitHub Issues page. Helpful details: your Windows version, graphics
card, which version of Spore you own (Steam / EA / GOG / disc), what you did
and what you saw. If OpenSpore cannot find your Spore install, include the
output of:
  openspore.exe --detect-spore


License
-------
OpenSpore is free software under the GNU General Public License, version 3 or
later; see LICENSE. Spore is a trademark of Electronic Arts Inc. The game files
OpenSpore reads remain the property of their owners and are not included.
