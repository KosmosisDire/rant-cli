include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

rant(init)

rant(new group nav)
expect_match("${OUT}" "created nav[.]hcl")
file(READ "${SCRATCH}/nav.hcl" nav)
expect_match("${nav}" "description = \"What nav runs\"")
rant(new group nav FAILS)
expect_match("${ERR}" "nav[.]hcl exists already, nothing was written")

# A node takes the language of the package it is made in.
file(WRITE "${SCRATCH}/py/pyproject.toml" "[project]\nname = \"py\"\ndependencies = [\"rant-middleware\"]\n")
rant(new node camera IN "${SCRATCH}/py")
expect_match("${OUT}" "created camera[.]py")
rant(build --dry-run)
expect_match("${OUT}" "py/camera +python +py/camera[.]py")

rant(new node arm FAILS)
expect_match("${ERR}" "say which language with --lang")
rant(new node arm --lang rust FAILS)
expect_match("${ERR}" "unknown language `rust`")
rant(new thing x FAILS)
expect_match("${ERR}" "say what to make")

# A template of one's own: params in template.hcl, asked for when missing and stdin is a
# terminal, an error here since it is not.
file(WRITE "${SCRATCH}/tpl/template.hcl" [[
template {
  description = "A web panel"
  param "port" {
    type        = int
    description = "Where it listens"
  }
  param "theme" {
    default = "dark"
    options = ["dark", "light"]
  }
  next = "open http://localhost:{{ port }}"
}
]])
file(WRITE "${SCRATCH}/tpl/{{name}}/{{snake(name)}}.txt" "{{ pascal(name) }} on {{ port }} in {{ theme }}\n")
rant(new package "Web Panel" --template tpl --help)
expect_match("${OUT}" "A web panel")
expect_match("${OUT}" "port=<int> +required +Where it listens")
expect_match("${OUT}" "theme=<string> +default dark, one of dark, light")
rant(new package web --template tpl FAILS)
expect_match("${ERR}" "required param `port` is not set")
rant(new package web --template tpl port=80 theme=pink FAILS)
expect_match("${ERR}" "must be one of dark, light")
rant(new node "Web Panel" --template tpl port=8080)
expect_match("${ERR}" "open http://localhost:8080")
file(READ "${SCRATCH}/Web Panel/web_panel.txt" made)
expect_match("${made}" "^WebPanel on 8080 in dark\n$")

rant(new node x --template nowhere FAILS)
expect_match("${ERR}" "no template `nowhere`")

# A package gets Rant through the same routine as rant lib install, which asks GitHub.
rant(new package cam --lang cpp MAY_FAIL)
expect_match("${OUT}" "created cam/CMakeLists[.]txt\ncreated cam/cam[.]cpp\n")
if(NOT CODE EQUAL 0 AND ERR MATCHES "cannot reach|rate limit")
  message("SKIPPED: GitHub is not reachable: ${ERR}")
  return()
elseif(NOT CODE EQUAL 0)
  message(FATAL_ERROR "rant new package cam exited ${CODE}\n${ERR}")
endif()
file(READ "${SCRATCH}/cam/CMakeLists.txt" cam)
expect_match("${cam}" "FetchContent_MakeAvailable[(]rant[)]")
expect_match("${cam}" "add_executable[(]cam cam[.]cpp[)]\ntarget_link_libraries[(]cam PRIVATE rant::rant_host[)]\n")
rant(lib cam)
expect_match("${OUT}" "cam +cmake +[0-9]+[.][0-9]+[.][0-9]+ +FetchContent")
