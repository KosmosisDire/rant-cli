#pragma once

#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace app {

/* One option a command accepts. An empty value_name makes it a flag. */
struct OptionSpec {
    std::string_view name;          /* "json" for --json */
    char             short_name;    /* 'y' for -y, 0 for none */
    std::string_view value_name;    /* "N" for --domain N, empty for a flag */
    std::string_view help;
};

/* The words and options after the command name. Options may appear anywhere, "--" ends
 * them, and a negative number such as -1 is a word, not an option. */
class Args {
public:
    std::vector<std::string> words;

    bool has(std::string_view name) const { return options_.count(std::string(name)) != 0; }
    std::optional<std::string> get(std::string_view name) const;
    void set(std::string_view name, std::string value) { options_[std::string(name)] = std::move(value); }
    void merge(const Args& other);

private:
    std::map<std::string, std::string> options_;
};

/* Parses tokens against specs. With stop_at_word the parse ends at the first word and
 * *consumed says how many tokens it took, which is how the command name is found. */
Args parse_args(const std::vector<std::string>& tokens, const std::vector<OptionSpec>& specs,
                bool stop_at_word = false, size_t* consumed = nullptr);

/* "1", "-2", "-0.5": a word even with a leading dash. */
bool looks_like_number(std::string_view s);

}
