/* The POSIX backend of process/command.hpp and process/supervisor.hpp. */

#include <cerrno>
#include <cstdint>
#include <csignal>
#include <cstring>
#include <fstream>
#include <optional>
#include <sstream>
#include <thread>

#include <fcntl.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

#ifdef __APPLE__
#include <libproc.h>
#include <mach-o/dyld.h>
#endif

#include "app/failure.hpp"
#include "process/supervisor.hpp"

namespace process {

fs::path self_path() {
#ifdef __APPLE__
    uint32_t size = 0;
    _NSGetExecutablePath(nullptr, &size);
    std::string buf(size, '\0');
    if (_NSGetExecutablePath(buf.data(), &size) == 0) return fs::weakly_canonical(fs::path(buf.c_str()));
    return {};
#else
    std::error_code ec;
    return fs::read_symlink("/proc/self/exe", ec);
#endif
}

/* Everything exec needs, built before fork: the child may only make async signal safe
 * calls, since other threads (the Rant node's) hold locks at the moment of the fork. */
struct Prepared {
    std::string              program;
    std::string              cwd;
    std::vector<std::string> args;
    std::vector<std::string> env;
    std::vector<char*>       argv_p;
    std::vector<char*>       env_p;
};

static Prepared prepare(const Command& c) {
    fs::path program = require_program(c);
    Prepared p;
    p.program = program.string();
    p.cwd = c.cwd.string();
    p.args = c.argv;
    p.env = environment(c.env);
    for (auto& a : p.args) p.argv_p.push_back(a.data());
    p.argv_p.push_back(nullptr);
    for (auto& e : p.env) p.env_p.push_back(e.data());
    p.env_p.push_back(nullptr);
    return p;
}

/* In the child, after the stdio fds are in place: every other fd but `keep` goes, so no
 * lock, socket or pipe of the CLI leaks into a node. */
static void close_other_fds(int keep) {
#if defined(__linux__) && defined(SYS_close_range)
    if (syscall(SYS_close_range, (unsigned)keep + 1, ~0u, 0) == 0) {
        for (int fd = 3; fd < keep; fd++) close(fd);
        return;
    }
#endif
    long max = sysconf(_SC_OPEN_MAX);
    if (max < 0 || max > 65536) max = 65536;
    for (int fd = 3; fd < max; fd++)
        if (fd != keep) close(fd);
}

/* Forks and execs. A failed chdir or exec comes back through a close on exec pipe, so the
 * caller learns of it rather than reading an exit code. */
static pid_t fork_exec(const Command& c, bool detached, int out_fd) {
    Prepared p = prepare(c);
    int nul = detached ? open("/dev/null", O_RDONLY | O_CLOEXEC) : -1;
    int pipefd[2];
    if (pipe(pipefd) != 0) throw app::Failure(std::string("pipe: ") + std::strerror(errno));
    fcntl(pipefd[0], F_SETFD, FD_CLOEXEC);
    fcntl(pipefd[1], F_SETFD, FD_CLOEXEC);

    pid_t pid = fork();
    if (pid == 0) {
        sigset_t none;
        sigemptyset(&none);
        sigprocmask(SIG_SETMASK, &none, nullptr);
        if (detached) {
            setsid();
            dup2(nul, 0);
            dup2(out_fd, 1);
            dup2(out_fd, 2);
        } else if (out_fd >= 0) {
            dup2(out_fd, 1);
        }
        int report = fcntl(pipefd[1], F_DUPFD_CLOEXEC, 3);
        close_other_fds(report);
        int stage = 0;
        if (chdir(p.cwd.c_str()) == 0) {
            stage = 1;
            execve(p.program.c_str(), p.argv_p.data(), p.env_p.data());
        }
        int msg[2] = { stage, errno };
        ssize_t w = write(report, msg, sizeof msg);
        (void)w;
        _exit(127);
    }
    int saved = errno;
    close(pipefd[1]);
    if (nul >= 0) close(nul);
    if (pid < 0) {
        close(pipefd[0]);
        throw app::Failure(std::string("fork: ") + std::strerror(saved));
    }
    int msg[2];
    ssize_t n;
    do n = read(pipefd[0], msg, sizeof msg); while (n < 0 && errno == EINTR);
    close(pipefd[0]);
    if (n == (ssize_t)sizeof msg) {
        waitpid(pid, nullptr, 0);
        if (msg[0] == 0) throw app::Failure("cannot enter " + p.cwd + ": " + std::strerror(msg[1]));
        throw app::Failure("cannot run " + p.program + ": " + std::strerror(msg[1]));
    }
    return pid;
}

int run(const Command& c, const fs::path& out_file) {
    int out = -1;
    if (!out_file.empty()) {
        out = open(out_file.c_str(), O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
        if (out < 0) throw app::Failure("cannot write " + out_file.string() + ": " + std::strerror(errno));
    }
    pid_t pid;
    try {
        pid = fork_exec(c, false, out);
    } catch (...) {
        if (out >= 0) close(out);
        throw;
    }
    if (out >= 0) close(out);
    int status = 0;
    while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {}
    if (WIFEXITED(status)) return WEXITSTATUS(status);
    if (WIFSIGNALED(status)) return 128 + WTERMSIG(status);
    return 1;
}

/* When the process started, in a unit the OS keeps for its life: clock ticks since boot on
 * Linux, microseconds since the epoch on macOS. nullopt when it is gone. */
static std::optional<uint64_t> start_time(pid_t pid) {
#ifdef __APPLE__
    struct proc_bsdinfo info;
    if (proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, sizeof info) != (int)sizeof info) return std::nullopt;
    return (uint64_t)info.pbi_start_tvsec * 1000000u + info.pbi_start_tvusec;
#else
    std::ifstream f("/proc/" + std::to_string(pid) + "/stat");
    std::string text((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
    size_t close_paren = text.rfind(')');    /* the name may hold spaces and parentheses */
    if (close_paren == std::string::npos) return std::nullopt;
    std::istringstream rest(text.substr(close_paren + 2));
    std::string field;
    for (int i = 3; i <= 22 && rest >> field; i++)
        if (i == 22) return std::stoull(field);
    return std::nullopt;
#endif
}

Tracking start_detached(const Command& c, const fs::path& log, const std::string&) {
    int fd = open(log.c_str(), O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
    if (fd < 0) throw app::Failure("cannot open " + log.string() + ": " + std::strerror(errno));
    pid_t pid;
    try {
        pid = fork_exec(c, true, fd);
    } catch (...) {
        close(fd);
        throw;
    }
    close(fd);
    Tracking t;
    t.pid = (uint64_t)pid;
    t.start_time = start_time(pid).value_or(0);
    return t;
}

/* Collects our own exited child, so a node started in this same run never lingers as a
 * zombie that still counts as a group member. Not our child: nothing happens. */
static void reap(pid_t pid) { waitpid(pid, nullptr, WNOHANG); }

/* kill(-0) would signal our own group and kill(-1) every process we may signal, so a
 * tracked id below 2, which no started node can have, is never used. */
static bool trackable(const Tracking& t) { return t.pid > 1 && t.pid <= (uint64_t)INT32_MAX; }

bool alive(const Tracking& t) {
    if (!trackable(t)) return false;
    pid_t pgid = (pid_t)t.pid;
    reap(pgid);
    if (kill(-pgid, 0) != 0 && errno != EPERM) return false;
    /* The group id stays taken while any member lives. Only a leader that exists with
     * another start time means the id was given to a new group. */
    auto st = start_time(pgid);
    return !st || !t.start_time || *st == t.start_time;
}

bool owns(const Tracking& t, uint64_t pid) {
    return alive(t) && getpgid((pid_t)pid) == (pid_t)t.pid;
}

static bool wait_gone(const Tracking& t, std::chrono::milliseconds limit) {
    auto deadline = std::chrono::steady_clock::now() + limit;
    while (alive(t)) {
        if (std::chrono::steady_clock::now() >= deadline) return false;
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    return true;
}

Stopped stop(const Tracking& t, std::chrono::milliseconds grace) {
    if (!alive(t)) return Stopped::AlreadyGone;    /* also every untrackable id */
    kill(-(pid_t)t.pid, SIGTERM);
    if (wait_gone(t, grace)) return Stopped::Gracefully;
    kill(-(pid_t)t.pid, SIGKILL);
    wait_gone(t, std::chrono::seconds(2));
    return Stopped::Killed;
}

Stopped stop_pid(uint64_t pid, std::chrono::milliseconds grace) {
    /* 0 and -1 would signal a whole group, and this process must not stop itself */
    if (pid <= 1 || pid > (uint64_t)INT32_MAX || (pid_t)pid == getpid()) return Stopped::AlreadyGone;
    pid_t p = (pid_t)pid;
    if (kill(p, SIGTERM) != 0) {
        if (errno == EPERM) throw app::Failure("process " + std::to_string(pid) + " belongs to another user, so rant may not stop it");
        return Stopped::AlreadyGone;
    }
    auto deadline = std::chrono::steady_clock::now() + grace;
    while (kill(p, 0) == 0) {
        if (std::chrono::steady_clock::now() >= deadline) {
            kill(p, SIGKILL);
            return Stopped::Killed;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    return Stopped::Gracefully;
}

std::optional<int> helper_main(int, char**) { return std::nullopt; }

}
