/* The Windows backend of process/command.hpp and process/supervisor.hpp. */

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <algorithm>
#include <cstdlib>
#include <memory>
#include <stdexcept>
#include <thread>

#include "app/failure.hpp"
#include "process/quote.hpp"
#include "process/supervisor.hpp"

namespace process {

static std::wstring wide(const std::string& s) {
    if (s.empty()) return {};
    int n = MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0);
    std::wstring w(n, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), w.data(), n);
    return w;
}

static std::string last_error_text() {
    DWORD code = GetLastError();
    wchar_t* buf = nullptr;
    FormatMessageW(FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
                   nullptr, code, 0, (wchar_t*)&buf, 0, nullptr);
    std::string s;
    if (buf) {
        int n = WideCharToMultiByte(CP_UTF8, 0, buf, -1, nullptr, 0, nullptr, nullptr);
        s.resize(n > 0 ? n - 1 : 0);
        if (n > 1) WideCharToMultiByte(CP_UTF8, 0, buf, -1, s.data(), n, nullptr, nullptr);
        LocalFree(buf);
    }
    while (!s.empty() && (s.back() == '\n' || s.back() == '\r' || s.back() == '.')) s.pop_back();
    return s.empty() ? "error " + std::to_string(code) : s;
}

struct HandleCloser {
    void operator()(HANDLE h) const {
        if (h && h != INVALID_HANDLE_VALUE) CloseHandle(h);
    }
};
using Handle = std::unique_ptr<void, HandleCloser>;

/* What CreateProcessW takes: the program path and the one command line string. A batch
 * file runs through cmd.exe with arguments escaped so cmd.exe passes them on as data. */
struct Launch {
    std::wstring app;
    std::wstring cmdline;
};

static Launch launch_for(const Command& c) {
    if (c.argv.empty()) throw app::Failure("empty command");
    auto program = find_program(c.argv[0], c.cwd);
    if (!program) throw app::Failure("`" + c.argv[0] + "` was not found on PATH");
    std::string ext = program->extension().u8string();
    std::transform(ext.begin(), ext.end(), ext.begin(), [](char ch) { return (char)tolower((unsigned char)ch); });
    std::vector<std::string> args(c.argv.begin() + 1, c.argv.end());
    if (ext == ".bat" || ext == ".cmd") {
        wchar_t sys[MAX_PATH];
        UINT n = GetSystemDirectoryW(sys, MAX_PATH);
        try {
            return { std::wstring(sys, n) + L"\\cmd.exe", wide(batch_command_line(program->u8string(), args)) };
        } catch (const std::invalid_argument& e) {
            throw app::Failure(std::string("cannot run ") + program->u8string() + ": " + e.what());
        }
    }
    std::vector<std::string> argv = c.argv;
    argv[0] = program->u8string();
    return { program->wstring(), wide(windows_command_line(argv)) };
}

/* A double NUL terminated UTF-16 block, sorted as CreateProcess expects. */
static std::wstring env_block(const Command& c) {
    std::wstring block;
    for (auto& e : environment(c.env)) {
        block += wide(e);
        block.push_back(L'\0');
    }
    block.push_back(L'\0');
    return block;
}

static SECURITY_ATTRIBUTES inheritable() {
    SECURITY_ATTRIBUTES sa{};
    sa.nLength = sizeof sa;
    sa.bInheritHandle = TRUE;
    return sa;
}

static Handle open_null(DWORD access) {
    SECURITY_ATTRIBUTES sa = inheritable();
    return Handle(CreateFileW(L"NUL", access, FILE_SHARE_READ | FILE_SHARE_WRITE, &sa, OPEN_EXISTING, 0, nullptr));
}

/* CreateProcessW where the child inherits exactly the handles listed, never a lock file or
 * socket the CLI happens to hold. Each listed handle must be inheritable. */
static PROCESS_INFORMATION create(const Command& c, DWORD flags, HANDLE in, HANDLE out, HANDLE err,
                                  std::vector<HANDLE> extra) {
    Launch l = launch_for(c);
    std::wstring env = env_block(c);
    std::wstring cwd = c.cwd.wstring();

    std::vector<HANDLE> list = { in, out, err };
    list.insert(list.end(), extra.begin(), extra.end());
    std::sort(list.begin(), list.end());
    list.erase(std::unique(list.begin(), list.end()), list.end());

    SIZE_T size = 0;
    InitializeProcThreadAttributeList(nullptr, 1, 0, &size);
    std::vector<char> attr_mem(size);
    auto attrs = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(attr_mem.data());
    if (!InitializeProcThreadAttributeList(attrs, 1, 0, &size) ||
        !UpdateProcThreadAttribute(attrs, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, list.data(),
                                   list.size() * sizeof(HANDLE), nullptr, nullptr))
        throw app::Failure("cannot set up the process: " + last_error_text());

    STARTUPINFOEXW si{};
    si.StartupInfo.cb = sizeof si;
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    si.StartupInfo.hStdInput = in;
    si.StartupInfo.hStdOutput = out;
    si.StartupInfo.hStdError = err;
    si.lpAttributeList = attrs;

    PROCESS_INFORMATION pi{};
    std::vector<wchar_t> cmdline(l.cmdline.begin(), l.cmdline.end());
    cmdline.push_back(L'\0');
    DWORD all = flags | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;
    BOOL ok = CreateProcessW(l.app.c_str(), cmdline.data(), nullptr, nullptr, TRUE, all, env.data(),
                             cwd.empty() ? nullptr : cwd.c_str(), &si.StartupInfo, &pi);
    /* A terminal may run us inside a job that would kill our nodes when it closes, so a
     * detached start tries to leave it first, and stays when the job forbids leaving. */
    if (!ok && (flags & CREATE_BREAKAWAY_FROM_JOB) && GetLastError() == ERROR_ACCESS_DENIED)
        ok = CreateProcessW(l.app.c_str(), cmdline.data(), nullptr, nullptr, TRUE, all & ~CREATE_BREAKAWAY_FROM_JOB,
                            env.data(), cwd.empty() ? nullptr : cwd.c_str(), &si.StartupInfo, &pi);
    DeleteProcThreadAttributeList(attrs);
    if (!ok) throw app::Failure("cannot run " + c.argv[0] + ": " + last_error_text());
    return pi;
}

/* An inheritable copy of a std handle, or the null device when there is none. */
static Handle std_copy(DWORD which, DWORD null_access) {
    HANDLE h = GetStdHandle(which);
    HANDLE dup = nullptr;
    if (h && h != INVALID_HANDLE_VALUE &&
        DuplicateHandle(GetCurrentProcess(), h, GetCurrentProcess(), &dup, 0, TRUE, DUPLICATE_SAME_ACCESS))
        return Handle(dup);
    return open_null(null_access);
}

int run(const Command& c, const fs::path& out_file) {
    Handle in = std_copy(STD_INPUT_HANDLE, GENERIC_READ);
    Handle out;
    if (out_file.empty()) {
        out = std_copy(STD_OUTPUT_HANDLE, GENERIC_WRITE);
    } else {
        SECURITY_ATTRIBUTES sa = inheritable();
        out = Handle(CreateFileW(out_file.wstring().c_str(), GENERIC_WRITE, FILE_SHARE_READ, &sa, CREATE_ALWAYS, 0, nullptr));
        if (out.get() == INVALID_HANDLE_VALUE) throw app::Failure("cannot write " + out_file.u8string() + ": " + last_error_text());
    }
    Handle err = std_copy(STD_ERROR_HANDLE, GENERIC_WRITE);
    PROCESS_INFORMATION pi = create(c, 0, in.get(), out.get(), err.get(), {});
    CloseHandle(pi.hThread);
    WaitForSingleObject(pi.hProcess, INFINITE);
    DWORD code = 1;
    GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(pi.hProcess);
    return (int)code;
}

static uint64_t creation_time(HANDLE process) {
    FILETIME created{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(process, &created, &exited, &kernel, &user)) return 0;
    return ((uint64_t)created.dwHighDateTime << 32) | created.dwLowDateTime;
}

Tracking start_detached(const Command& c, const fs::path& log, const std::string& job_name) {
    SECURITY_ATTRIBUTES sa = inheritable();
    Handle out(CreateFileW(log.wstring().c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                           &sa, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr));
    if (out.get() == INVALID_HANDLE_VALUE) throw app::Failure("cannot open " + log.u8string() + ": " + last_error_text());
    Handle in = open_null(GENERIC_READ);

    /* The node inherits a handle to its own job. A named object loses its name when its
     * last handle closes, and this handle keeps the name findable after the CLI exits. */
    Handle job(CreateJobObjectW(&sa, wide(job_name).c_str()));
    if (!job) throw app::Failure("cannot create job " + job_name + ": " + last_error_text());
    if (GetLastError() == ERROR_ALREADY_EXISTS)
        throw app::Failure("job " + job_name + " already exists: an earlier start of this node still runs");

    DWORD flags = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_SUSPENDED | CREATE_BREAKAWAY_FROM_JOB;
    PROCESS_INFORMATION pi = create(c, flags, in.get(), out.get(), out.get(), { job.get() });
    if (!AssignProcessToJobObject(job.get(), pi.hProcess)) {
        std::string why = last_error_text();
        TerminateProcess(pi.hProcess, 1);
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        throw app::Failure("cannot track " + c.argv[0] + " in a job: " + why);
    }
    ResumeThread(pi.hThread);
    Tracking t;
    t.pid = pi.dwProcessId;
    t.start_time = creation_time(pi.hProcess);
    t.job = job_name;
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    return t;
}

static Handle open_job(const Tracking& t, DWORD access) {
    return Handle(OpenJobObjectW(access, FALSE, wide(t.job).c_str()));
}

static DWORD active_processes(HANDLE job) {
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION info{};
    if (!QueryInformationJobObject(job, JobObjectBasicAccountingInformation, &info, sizeof info, nullptr)) return 0;
    return info.ActiveProcesses;
}

bool alive(const Tracking& t) {
    Handle job = open_job(t, JOB_OBJECT_QUERY);
    return job && active_processes(job.get()) > 0;
}

bool owns(const Tracking& t, uint64_t pid) {
    Handle job = open_job(t, JOB_OBJECT_QUERY);
    Handle proc(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, (DWORD)pid));
    BOOL in = FALSE;
    return job && proc && IsProcessInJob(proc.get(), job.get(), &in) && in;
}

/* The ids of the processes in the job, for a console to attach to when the first one is
 * gone. */
static std::vector<DWORD> job_pids(HANDLE job) {
    std::vector<char> buf(sizeof(JOBOBJECT_BASIC_PROCESS_ID_LIST) + 64 * sizeof(ULONG_PTR));
    auto list = reinterpret_cast<JOBOBJECT_BASIC_PROCESS_ID_LIST*>(buf.data());
    std::vector<DWORD> out;
    if (QueryInformationJobObject(job, JobObjectBasicProcessIdList, list, (DWORD)buf.size(), nullptr))
        for (DWORD i = 0; i < list->NumberOfProcessIdsInList; i++) out.push_back((DWORD)list->ProcessIdList[i]);
    return out;
}

static const wchar_t* HELPER_FLAG = L"--rant-ctrl-break";

fs::path self_path() {
    std::wstring buf(MAX_PATH, L'\0');
    for (;;) {    /* a path longer than the buffer comes back cut, so grow until it fits */
        DWORD n = GetModuleFileNameW(nullptr, buf.data(), (DWORD)buf.size());
        if (n < buf.size()) return fs::path(buf.substr(0, n));
        buf.resize(buf.size() * 2);
    }
}

/* Runs this executable as the Ctrl-Break helper, with no console of its own, and waits.
 * True when the event went out. */
static bool send_ctrl_break(uint64_t group, uint64_t attach) {
    std::wstring self = self_path().wstring();
    std::wstring cmd = L"\"" + self + L"\" " + HELPER_FLAG + L" " + std::to_wstring(group) + L" " + std::to_wstring(attach);
    STARTUPINFOW si{};
    si.cb = sizeof si;
    PROCESS_INFORMATION pi{};
    if (!CreateProcessW(self.c_str(), cmd.data(), nullptr, nullptr, FALSE, DETACHED_PROCESS, nullptr, nullptr, &si, &pi))
        return false;
    DWORD code = 1;
    if (WaitForSingleObject(pi.hProcess, 3000) == WAIT_OBJECT_0) GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    return code == 0;
}

std::optional<int> helper_main(int argc, char** argv) {
    if (argc != 4 || std::string(argv[1]) != "--rant-ctrl-break") return std::nullopt;
    DWORD group = (DWORD)std::strtoul(argv[2], nullptr, 10);
    DWORD attach_pid = (DWORD)std::strtoul(argv[3], nullptr, 10);
    FreeConsole();
    if (!AttachConsole(attach_pid)) return 1;
    SetConsoleCtrlHandler(nullptr, TRUE);    /* the event must not stop this helper first */
    BOOL ok = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, group);
    FreeConsole();
    return ok ? 0 : 1;
}

static bool wait_gone(const Tracking& t, std::chrono::milliseconds limit) {
    auto deadline = std::chrono::steady_clock::now() + limit;
    while (alive(t)) {
        if (std::chrono::steady_clock::now() >= deadline) return false;
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    return true;
}

/* Sends Ctrl-Break until it goes out, then waits for the job to empty, all within grace.
 * A process stopped right after it started may not have its console yet, so the first
 * attach can fail. */
static bool ask_to_stop(const Tracking& t, HANDLE job, std::chrono::milliseconds grace) {
    auto deadline = std::chrono::steady_clock::now() + grace;
    bool sent = false;
    while (alive(t)) {
        if (std::chrono::steady_clock::now() >= deadline) return false;
        if (!sent) {
            auto pids = job_pids(job);
            bool first = std::find(pids.begin(), pids.end(), (DWORD)t.pid) != pids.end();
            sent = send_ctrl_break(t.pid, first || pids.empty() ? t.pid : pids[0]);
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(sent ? 20 : 50));
    }
    return true;
}

Stopped stop_pid(uint64_t pid, std::chrono::milliseconds) {
    if (pid == 0 || pid == GetCurrentProcessId()) return Stopped::AlreadyGone;
    Handle h(OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, FALSE, (DWORD)pid));
    if (!h && GetLastError() == ERROR_ACCESS_DENIED)
        throw app::Failure("process " + std::to_string(pid) + " belongs to another user, so rant may not stop it");
    if (!h || !TerminateProcess(h.get(), 1)) return Stopped::AlreadyGone;
    WaitForSingleObject(h.get(), 2000);
    return Stopped::KilledAtOnce;
}

Stopped stop(const Tracking& t, std::chrono::milliseconds grace) {
    Handle job = open_job(t, JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE);
    if (!job || active_processes(job.get()) == 0) return Stopped::AlreadyGone;
    if (ask_to_stop(t, job.get(), grace)) return Stopped::Gracefully;
    TerminateJobObject(job.get(), 1);
    wait_gone(t, std::chrono::seconds(2));
    return Stopped::Killed;
}

}
