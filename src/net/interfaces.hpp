#pragma once

#include <string>
#include <vector>

namespace net {

/* The IP addresses of this machine's network interfaces as text, loopback included. */
std::vector<std::string> local_addresses();

}
