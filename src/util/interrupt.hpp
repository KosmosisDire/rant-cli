#pragma once

namespace util {

/* Catches Ctrl-C from here on, so a long running command can stop cleanly. */
void catch_interrupt();

/* True once Ctrl-C was pressed after catch_interrupt. */
bool interrupted();

}
