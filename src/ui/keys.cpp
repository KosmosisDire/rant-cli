#include "ui/keys.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <poll.h>
#include <termios.h>
#include <unistd.h>
#endif

namespace ui {

static Key from_char(int c) {
    switch (c) {
    case 'k': return Key::Up;
    case 'j': return Key::Down;
    case ' ': return Key::Toggle;
    case 'a': return Key::All;
    case '\r':
    case '\n': return Key::Enter;
    case 'q':
    case 3:    /* Ctrl-C */
    case 4:    /* Ctrl-D */
    case 27: return Key::Quit;
    }
    return Key::Other;
}

#ifdef _WIN32

KeyReader::KeyReader() {
    HANDLE in = GetStdHandle(STD_INPUT_HANDLE);
    DWORD mode = 0;
    if (!GetConsoleMode(in, &mode)) return;
    saved_ = mode;
    /* virtual key codes rather than escape sequences, and Ctrl-C as a key */
    mode &= ~(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
    ok_ = SetConsoleMode(in, mode) != 0;
}

KeyReader::~KeyReader() {
    if (ok_) SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), (DWORD)saved_);
}

Key KeyReader::next() {
    HANDLE in = GetStdHandle(STD_INPUT_HANDLE);
    for (;;) {
        INPUT_RECORD rec;
        DWORD n = 0;
        if (!ReadConsoleInputW(in, &rec, 1, &n) || n == 0) return Key::Quit;
        if (rec.EventType != KEY_EVENT || !rec.Event.KeyEvent.bKeyDown) continue;
        switch (rec.Event.KeyEvent.wVirtualKeyCode) {
        case VK_UP: return Key::Up;
        case VK_DOWN: return Key::Down;
        }
        wchar_t c = rec.Event.KeyEvent.uChar.UnicodeChar;
        if (c) return from_char(c);
    }
}

#else

KeyReader::KeyReader() {
    struct termios t;
    if (!isatty(STDIN_FILENO) || tcgetattr(STDIN_FILENO, &t) != 0) return;
    termios_ = new struct termios(t);
    t.c_lflag &= ~(ICANON | ECHO | ISIG);
    t.c_cc[VMIN] = 1;
    t.c_cc[VTIME] = 0;
    ok_ = tcsetattr(STDIN_FILENO, TCSANOW, &t) == 0;
}

KeyReader::~KeyReader() {
    if (!termios_) return;
    tcsetattr(STDIN_FILENO, TCSANOW, static_cast<struct termios*>(termios_));
    delete static_cast<struct termios*>(termios_);
}

/* A byte that follows within 50 ms, -1 when none does, as after a lone Esc. */
static int follow() {
    struct pollfd p = { STDIN_FILENO, POLLIN, 0 };
    unsigned char c;
    if (poll(&p, 1, 50) <= 0 || read(STDIN_FILENO, &c, 1) != 1) return -1;
    return c;
}

Key KeyReader::next() {
    unsigned char c;
    if (read(STDIN_FILENO, &c, 1) != 1) return Key::Quit;
    if (c != 27) return from_char(c);
    int b = follow();
    if (b != '[' && b != 'O') return b < 0 ? Key::Quit : Key::Other;
    switch (follow()) {
    case 'A': return Key::Up;
    case 'B': return Key::Down;
    }
    return Key::Other;
}

#endif

}
