#include "check.hpp"
#include "ui/checklist.hpp"

using ui::Key;

static ui::Checklist three() { return ui::Checklist("Pick", { { "a", "first" }, { "b", "second" }, { "c", "" } }); }

TEST(checklist_moves_with_wrap_and_ticks) {
    ui::Checklist c = three();
    c.press(Key::Up);    /* from the first row up to the last */
    c.press(Key::Toggle);
    c.press(Key::Down);
    c.press(Key::Down);
    c.press(Key::Toggle);
    CHECK(!c.press(Key::Enter));
    CHECK(c.confirmed());
    CHECK(c.ticked() == (std::vector<size_t>{ 1, 2 }));
}

TEST(checklist_a_ticks_all_then_none) {
    ui::Checklist c = three();
    c.press(Key::Toggle);
    c.press(Key::All);
    CHECK(c.ticked().size() == 3);
    c.press(Key::All);
    CHECK(c.ticked().empty());
}

TEST(checklist_quit_confirms_nothing) {
    ui::Checklist c = three();
    c.press(Key::Toggle);
    CHECK(!c.press(Key::Quit));
    CHECK(c.done());
    CHECK(!c.confirmed());
}

TEST(checklist_draws_aligned_rows_then_the_pick) {
    ui::Output out;    /* no colors, since the test's output is no terminal */
    ui::Checklist c("Install", { { "bridge", "web" }, { "hmi", "panels" } });
    c.press(Key::Toggle);
    auto lines = c.lines(out);
    CHECK_EQ(lines.size(), size_t(4));
    CHECK_EQ(lines[0], std::string("? Install"));
    CHECK_EQ(lines[2], std::string("> [x] bridge  web"));
    CHECK_EQ(lines[3], std::string("  [ ] hmi     panels"));
    c.press(Key::Enter);
    CHECK_EQ(c.lines(out), std::vector<std::string>{ "? Install  bridge" });
}
