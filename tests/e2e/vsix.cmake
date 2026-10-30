include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# A fake editor stands in for VS Code, so no test touches a real one. It logs its arguments
# and keeps a listing the way `code --list-extensions --show-versions` prints one.
set(home "$ENV{RANT_HOME}")
set(bin "${SCRATCH}/editor")
file(MAKE_DIRECTORY "${bin}")
if(CMAKE_HOST_WIN32)
  set(editor "${bin}/fake-code.cmd")
  file(WRITE "${editor}" "@echo off\r\n"
    "echo %*>> \"%~dp0calls.txt\"\r\n"
    "if \"%~1\"==\"--install-extension\" echo test.ext@0.0.1> \"%~dp0listing.txt\"\r\n"
    "if \"%~1\"==\"--uninstall-extension\" del \"%~dp0listing.txt\"\r\n"
    "if \"%~1\"==\"--list-extensions\" if exist \"%~dp0listing.txt\" type \"%~dp0listing.txt\"\r\n"
    "exit /b 0\r\n")
else()
  set(editor "${bin}/fake-code")
  file(WRITE "${editor}" "#!/bin/sh\n"
    "dir=$(dirname \"$0\")\n"
    "echo \"$*\" >> \"$dir/calls.txt\"\n"
    "case \"$1\" in\n"
    "  --install-extension) echo test.ext@0.0.1 > \"$dir/listing.txt\" ;;\n"
    "  --uninstall-extension) rm -f \"$dir/listing.txt\" ;;\n"
    "  --list-extensions) cat \"$dir/listing.txt\" 2>/dev/null ;;\n"
    "esac\n"
    "exit 0\n")
  file(CHMOD "${editor}" PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE)
endif()

# An extension's build folder: its package.rant and the .vsix its build wrote, a zip whose
# extension/package.json names the extension.
set(build "${SCRATCH}/ext-build")
file(WRITE "${build}/package.rant" "name = \"ext\"\ndescription = \"A test extension\"\n")
file(WRITE "${SCRATCH}/zip/extension/package.json" "{ \"name\": \"ext\", \"publisher\": \"Test\", \"version\": \"0.0.1\" }\n")
execute_process(COMMAND "${CMAKE_COMMAND}" -E tar cf "${build}/ext-0.0.1.vsix" --format=zip extension
                WORKING_DIRECTORY "${SCRATCH}/zip" RESULT_VARIABLE packed)
if(NOT packed EQUAL 0)
  message(FATAL_ERROR "cannot pack the test extension")
endif()

# The editor options belong to an extension, and a missing editor is refused before anything
# is installed.
file(WRITE "${SCRATCH}/plain/package.rant" "name = \"plain\"\ndescription = \"No extension\"\n")
rant(install plain --from plain --editor "${editor}" FAILS)
expect_match("${ERR}" "plain is no VS Code extension, so --editor and --profile do not apply")
rant(install ext --from ext-build --editor no-such-editor-here FAILS)
expect_match("${ERR}" "no editor `no-such-editor-here` found")
if(EXISTS "${home}/packages")
  file(GLOB left "${home}/packages/*")
  if(left)
    message(FATAL_ERROR "a refused install left ${left}")
  endif()
endif()

# Installing goes through the editor and records where, beside the linked folder.
rant(install ext --from ext-build --editor "${editor}")
expect_match("${ERR}" "installed the extension test[.]ext 0[.]0[.]1 through [^\n]*fake-code")
expect_match("${ERR}" "linked ext to")
file(READ "${bin}/calls.txt" calls)
expect_match("${calls}" "--install-extension [^\n]*ext-0[.]0[.]1[.]vsix\"? --force")
file(READ "${home}/packages/ext.json" record)
expect_match("${record}" "\"id\": \"test[.]ext\"")
rant(ls packages)
expect_match("${OUT}" "\n  ext +package[.]rant +linked")

# Remove takes it out of the recorded editor, and the build folder stays.
rant(remove ext)
expect_match("${ERR}" "removed ext")
file(READ "${bin}/calls.txt" calls)
expect_match("${calls}" "--uninstall-extension test[.]ext")
if(EXISTS "${home}/packages/ext" OR NOT EXISTS "${build}/ext-0.0.1.vsix")
  message(FATAL_ERROR "remove left the package or took the build")
endif()

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
rant(install ext --from ext-build --editor "${editor}" FAILS)
expect_match("${ERR}" "1  Default")
expect_match("${ERR}" "2  Mixed")
expect_no_match("${ERR}" "Agents")
expect_match("${ERR}" "pass --profile NAME")

# A VS Code terminal takes the profile its folder opens in, and remove the recorded one.
set(ENV{TERM_PROGRAM} "vscode")
rant(install ext --from "${build}" --editor "${editor}" IN "${SCRATCH}/project")
expect_match("${ERR}" "in the profile Mixed")
unset(ENV{TERM_PROGRAM})
rant(remove ext)
file(READ "${bin}/calls.txt" calls)
expect_match("${calls}" "--install-extension [^\n]* --force --profile Mixed")
expect_match("${calls}" "--uninstall-extension test[.]ext --profile Mixed")

# --profile picks one outright, and uninstall takes the extension out too.
rant(install ext --from ext-build --editor "${editor}" --profile Mixed)
expect_match("${ERR}" "in the profile Mixed")
set(ENV{HOME} "${SCRATCH}/home")
set(ENV{XDG_CONFIG_HOME} "${SCRATCH}/home/.config")
rant(uninstall)
expect_match("${OUT}" "the VS Code extension test[.]ext, through [^\n]*fake-code[^\n]* in the profile Mixed\n")
file(READ "${bin}/calls.txt" calls)
string(REGEX MATCHALL "--uninstall-extension" removals "${calls}")
list(LENGTH removals count)
if(NOT count EQUAL 2)
  message(FATAL_ERROR "uninstall did not remove the extension:\n${calls}")
endif()
