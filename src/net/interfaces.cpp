#include "net/interfaces.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <winsock2.h>
#include <ws2tcpip.h>
#include <iphlpapi.h>
#else
#include <arpa/inet.h>
#include <ifaddrs.h>
#include <netinet/in.h>
#endif

namespace net {

static void add(std::vector<std::string>& out, const sockaddr* sa) {
    char text[INET6_ADDRSTRLEN] = {};
    if (sa->sa_family == AF_INET)
        inet_ntop(AF_INET, &reinterpret_cast<const sockaddr_in*>(sa)->sin_addr, text, sizeof text);
    else if (sa->sa_family == AF_INET6)
        inet_ntop(AF_INET6, &reinterpret_cast<const sockaddr_in6*>(sa)->sin6_addr, text, sizeof text);
    if (text[0]) out.push_back(text);
}

std::vector<std::string> local_addresses() {
    std::vector<std::string> out = { "127.0.0.1", "::1" };
#ifdef _WIN32
    ULONG size = 16 * 1024;
    std::vector<unsigned char> buf;
    ULONG rc = ERROR_BUFFER_OVERFLOW;
    for (int tries = 0; tries < 3 && rc == ERROR_BUFFER_OVERFLOW; tries++) {
        buf.resize(size);
        rc = GetAdaptersAddresses(AF_UNSPEC, GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER,
                                  nullptr, reinterpret_cast<IP_ADAPTER_ADDRESSES*>(buf.data()), &size);
    }
    if (rc != NO_ERROR) return out;
    for (auto* a = reinterpret_cast<IP_ADAPTER_ADDRESSES*>(buf.data()); a; a = a->Next)
        for (auto* u = a->FirstUnicastAddress; u; u = u->Next) add(out, u->Address.lpSockaddr);
#else
    ifaddrs* list = nullptr;
    if (getifaddrs(&list) != 0) return out;
    for (ifaddrs* i = list; i; i = i->ifa_next)
        if (i->ifa_addr) add(out, i->ifa_addr);
    freeifaddrs(list);
#endif
    return out;
}

}
