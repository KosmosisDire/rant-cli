include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# A fake editor stands in for VS Code, so no test touches a real one. It logs its arguments
# and keeps a listing the way `code --list-extensions --show-versions` prints one.
set(home "${SCRATCH}/home/.rant")
set(bin "${SCRATCH}/editor")
file(MAKE_DIRECTORY "${bin}")
if(CMAKE_HOST_WIN32)
  set(editor "${bin}/fake-code.cmd")
  file(WRITE "${editor}" "@echo off\r\n"
    "echo %*>> \"%~dp0calls.txt\"\r\n"
    "if \"%~1\"==\"--install-extension\" echo kosmosisdire.vscode-rant@0.0.1> \"%~dp0listing.txt\"\r\n"
    "if \"%~1\"==\"--uninstall-extension\" del \"%~dp0listing.txt\"\r\n"
    "if \"%~1\"==\"--list-extensions\" if exist \"%~dp0listing.txt\" type \"%~dp0listing.txt\"\r\n"
    "exit /b 0\r\n")
else()
  set(editor "${bin}/fake-code")
  file(WRITE "${editor}" "#!/bin/sh\n"
    "dir=$(dirname \"$0\")\n"
    "echo \"$*\" >> \"$dir/calls.txt\"\n"
    "case \"$1\" in\n"
    "  --install-extension) echo kosmosisdire.vscode-rant@0.0.1 > \"$dir/listing.txt\" ;;\n"
    "  --uninstall-extension) rm -f \"$dir/listing.txt\" ;;\n"
    "  --list-extensions) cat \"$dir/listing.txt\" 2>/dev/null ;;\n"
    "esac\n"
    "exit 0\n")
  file(CHMOD "${editor}" PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE)
endif()

# Usage mistakes.
rant(vscode FAILS)
expect_match("${ERR}" "vscode needs a command")
rant(vscode bogus FAILS)
expect_match("${ERR}" "unknown vscode command `bogus`")
rant(vscode uninstall --from x.vsix FAILS)
expect_match("${ERR}" "--from goes with `rant vscode install`")

# A missing file or editor is refused before anything runs.
rant(vscode install --from missing.vsix FAILS)
expect_match("${ERR}" "no file `missing[.]vsix`")
file(WRITE "${SCRATCH}/ext.vsix" "not really a zip\n")
rant(vscode install --from ext.vsix --editor no-such-editor-here FAILS)
expect_match("${ERR}" "no editor `no-such-editor-here` found")

# No release to download: the proxy that refuses every connection stands in for no network
# or no release, so this holds once a release exists.
set(ENV{HTTPS_PROXY} "http://127.0.0.1:9")
set(ENV{https_proxy} "http://127.0.0.1:9")
rant(vscode install --editor "${editor}" FAILS)
expect_match("${ERR}" "rant vscode install --from PATH")
unset(ENV{HTTPS_PROXY})
unset(ENV{https_proxy})

# Installing a built extension goes through the editor and records it.
rant(vscode install --from ext.vsix --editor "${editor}")
expect_match("${ERR}" "installed the VS Code extension 0[.]0[.]1 through")
file(READ "${bin}/calls.txt" calls)
expect_match("${calls}" "--install-extension [^\n]*ext[.]vsix\"? --force")
if(NOT EXISTS "${home}/vscode.editor")
  message(FATAL_ERROR "the editor was not recorded")
endif()

# Uninstall uses the recorded editor and forgets it.
rant(vscode uninstall)
expect_match("${ERR}" "removed the VS Code extension")
file(READ "${bin}/calls.txt" calls)
expect_match("${calls}" "--uninstall-extension kosmosisdire[.]vscode-rant")
if(EXISTS "${home}/vscode.editor")
  message(FATAL_ERROR "the editor is still recorded after uninstall")
endif()
rant(vscode uninstall --editor "${editor}")
expect_match("${ERR}" "the VS Code extension is not installed")

# Profiles come from the editor's storage.json. The data folder of a portable install beside
# the fake editor holds it, so no test reads a real one.
unset(ENV{VSCODE_PORTABLE})
file(WRITE "${SCRATCH}/resources/app/product.json" "{ \"nameShort\": \"Fake\" }\n")
file(MAKE_DIRECTORY "${SCRATCH}/project")
if(CMAKE_HOST_WIN32)
  set(project_uri "file:///${SCRATCH}/project")
else()
  set(project_uri "file://${SCRATCH}/project")
endif()
file(WRITE "${SCRATCH}/data/user-data/User/globalStorage/storage.json" "{
  \"userDataProfiles\": [
    { \"location\": \"builtin/agents\", \"name\": \"Agents\", \"useDefaultFlags\": { \"extensions\": true } },
    { \"location\": \"-500e13d6\", \"name\": \"Mixed\" }
  ],
  \"profileAssociations\": { \"workspaces\": { \"${project_uri}\": \"-500e13d6\" } }
}\n")
file(REMOVE "${bin}/calls.txt")

# Outside a VS Code terminal it asks among the profiles with their own extensions, and with
# no one to answer it says how to pick.
unset(ENV{TERM_PROGRAM})
rant(vscode install --from ext.vsix --editor "${editor}" FAILS)
expect_match("${ERR}" "1  Default")
expect_match("${ERR}" "2  Mixed")
expect_no_match("${ERR}" "Agents")
expect_match("${ERR}" "pass --profile NAME")

# A VS Code terminal takes the profile its folder opens in, and uninstall the recorded one.
set(ENV{TERM_PROGRAM} "vscode")
rant(vscode install --from "${SCRATCH}/ext.vsix" --editor "${editor}" IN "${SCRATCH}/project")
expect_match("${ERR}" "in the profile Mixed")
unset(ENV{TERM_PROGRAM})
rant(vscode uninstall)
expect_match("${ERR}" "removed the VS Code extension from [^\n]* in the profile Mixed")
file(READ "${bin}/calls.txt" calls)
expect_match("${calls}" "--install-extension [^\n]* --force --profile Mixed")
expect_match("${calls}" "--uninstall-extension kosmosisdire[.]vscode-rant --profile Mixed")

# --profile picks one outright.
rant(vscode install --from ext.vsix --editor "${editor}" --profile Mixed)
expect_match("${ERR}" "in the profile Mixed")
