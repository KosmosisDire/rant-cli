#pragma once

#include <optional>
#include <string>
#include <string_view>

namespace ui {

/* Asks a yes or no question on stderr. assume_yes (from -y) answers yes at once, and so
 * does a stdin that is not a terminal, with the question still printed so logs show it. */
bool confirm(std::string_view question, bool assume_yes);

/* Asks for a line of text on stderr. nullopt when stdin is not a terminal or gives none. */
std::optional<std::string> ask(std::string_view question);

}
