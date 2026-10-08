#pragma once

#include <string_view>

namespace ui {

/* Asks a yes or no question on stderr. assume_yes (from -y) answers yes at once, and so
 * does a stdin that is not a terminal, with the question still printed so logs show it. */
bool confirm(std::string_view question, bool assume_yes);

}
