#include "process/quote.hpp"

#include <cctype>
#include <cstring>
#include <stdexcept>

namespace process {

/* MSVC rules: quote a word holding a space or tab, or an empty one. Backslashes double only
 * before a quote, and a quote inside is escaped with a backslash. */
static void append_arg(std::string& cmd, const std::string& arg) {
    bool quote = arg.empty() || arg.find_first_of(" \t") != std::string::npos;
    if (quote) cmd += '"';
    size_t backslashes = 0;
    for (char c : arg) {
        if (c == '\\') {
            backslashes++;
        } else {
            if (c == '"') cmd.append(backslashes + 1, '\\');
            backslashes = 0;
        }
        cmd += c;
    }
    if (quote) {
        cmd.append(backslashes, '\\');
        cmd += '"';
    }
}

std::string windows_command_line(const std::vector<std::string>& argv) {
    std::string cmd;
    for (size_t i = 0; i < argv.size(); i++) {
        if (i) cmd += ' ';
        append_arg(cmd, argv[i]);
    }
    return cmd;
}

/* cmd.exe rules: any ASCII mark but a few known safe ones forces quotes, a quote doubles,
 * and % becomes %%cd:~,% so cmd.exe cannot expand %VAR%: cd is always defined and the empty
 * substring of it expands to nothing. */
static void append_batch_arg(std::string& cmd, const std::string& arg) {
    if (arg.find_first_of("\r\n") != std::string::npos || arg.find('\0') != std::string::npos)
        throw std::invalid_argument("a batch file argument cannot hold a line break");
    static const char* safe = "#$*+-./:?@\\_";
    bool quote = arg.empty() || arg.back() == '\\';
    for (char c : arg) {
        unsigned char u = (unsigned char)c;
        if (u < 0x80 && !std::isalnum(u) && !std::strchr(safe, c)) quote = true;
    }
    if (quote) cmd += '"';
    size_t backslashes = 0;
    for (char c : arg) {
        if (c == '\\') {
            backslashes++;
        } else {
            if (c == '"') {
                cmd.append(backslashes, '\\');
                cmd += '"';
            } else if (c == '%') {
                cmd += "%%cd:~,";
            }
            backslashes = 0;
        }
        cmd += c;
    }
    if (quote) {
        cmd.append(backslashes, '\\');
        cmd += '"';
    }
}

std::string batch_command_line(const std::string& script, const std::vector<std::string>& args) {
    if (script.find('"') != std::string::npos || (!script.empty() && script.back() == '\\'))
        throw std::invalid_argument("a batch file name cannot hold a quote or end in a backslash");
    /* /e:ON keeps the %cd% trick working, /v:OFF leaves ! alone, /d skips AutoRun. The
     * outer quotes wrap the whole command for /c. */
    std::string cmd = "cmd.exe /e:ON /v:OFF /d /c \"\"" + script + "\"";
    for (auto& a : args) {
        cmd += ' ';
        append_batch_arg(cmd, a);
    }
    return cmd + "\"";
}

}
