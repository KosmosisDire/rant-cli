include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# A home set up the way the installers and setup leave it, beside the user's own lines.
set(home "${SCRATCH}/home")
set(ENV{HOME} "${home}")
set(ENV{XDG_CONFIG_HOME} "${home}/.config")
file(WRITE "${home}/.bashrc" "alias ll='ls -l'\n")
rant(setup bash)
file(APPEND "${home}/.bashrc" "\nexport PATH=\"${home}/.rant/bin:$PATH\"    # rant-cli\n")
file(WRITE "${home}/.config/fish/conf.d/rant.fish" "fish_add_path -g \"${home}/.rant/bin\"    # rant-cli\n")
file(WRITE "${home}/.config/fish/conf.d/other.fish" "set -g x 1\n")
file(MAKE_DIRECTORY "${home}/.claude")
rant(setup skill)
file(WRITE "${home}/.rant/bin/rant-explorer.version" "0.0.8\n")

rant(uninstall)
expect_match("${OUT}" "removing:\n")
expect_match("${OUT}" "[.]bashrc [(]its rant lines[)]\n")
expect_match("${OUT}" "conf[.]d.rant[.]fish\n")
expect_match("${OUT}" "[.]claude.skills.rant\n")
expect_match("${OUT}" "Workspaces stay as they are")
expect_match("${OUT}" "uninstalled rant-cli\n")

# Only rant's own lines and files went.
file(READ "${home}/.bashrc" bashrc)
expect_match("${bashrc}" "^alias ll='ls -l'\n$")
foreach(gone "${home}/.rant" "${home}/.claude/skills/rant" "${home}/.config/fish/conf.d/rant.fish")
  if(EXISTS "${gone}")
    message(FATAL_ERROR "${gone} is still there")
  endif()
endforeach()
foreach(kept "${home}/.claude" "${home}/.config/fish/conf.d/other.fish")
  if(NOT EXISTS "${kept}")
    message(FATAL_ERROR "${kept} was removed")
  endif()
endforeach()

rant(uninstall)
expect_match("${OUT}" "^rant-cli left nothing here to remove\n$")

# A RANT_HOME not named .rant is never deleted whole.
set(ENV{RANT_HOME} "${home}")
rant(uninstall FAILS)
expect_match("${ERR}" "uninstall only removes a folder named [.]rant")
