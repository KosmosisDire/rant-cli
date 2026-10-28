#pragma once

namespace ui {

/* The keys a picker understands. Ctrl-C arrives as Quit, never as a signal. */
enum class Key { Up, Down, Toggle, All, Enter, Quit, Other };

/* Single keys from the terminal on stdin, without echo or line editing, until it goes out of
 * scope and the terminal is as it was. ok() is false when stdin is no terminal. */
class KeyReader {
public:
    KeyReader();
    ~KeyReader();
    KeyReader(const KeyReader&) = delete;
    KeyReader& operator=(const KeyReader&) = delete;

    bool ok() const { return ok_; }

    /* Waits for the next key. A closed stdin reads as Quit. */
    Key next();

private:
    bool          ok_ = false;
    unsigned long saved_ = 0;    /* the console mode on Windows */
    void*         termios_ = nullptr;    /* the saved termios elsewhere */
};

}
