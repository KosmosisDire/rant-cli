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
