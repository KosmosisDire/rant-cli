#pragma once

#include <stdexcept>
#include <string>

namespace app {

/* A failed command. main prints the message as an error and exits with code. */
class Failure : public std::runtime_error {
public:
    explicit Failure(const std::string& message, int code = 1) : std::runtime_error(message), code_(code) {}
    int code() const { return code_; }

private:
    int code_;
};

/* A command line the CLI cannot make sense of, exit code 2. */
class UsageError : public Failure {
public:
    explicit UsageError(const std::string& message) : Failure(message, 2) {}
};

}
