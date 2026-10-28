#pragma once

#include <chrono>
#include <deque>
#include <optional>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "commands/format.hpp"
#include "rant.hpp"
#include "ui/live.hpp"

namespace commands {

/* Values a second over the last two seconds, measured across the span the values cover, so
 * it is right from the second value on. */
class Rate {
public:
    using Clock = std::chrono::steady_clock;
    void hit(Clock::time_point t) { times_.push_back(t); }
    double hz(Clock::time_point now);

private:
    std::deque<Clock::time_point> times_;
};

/* A stream of values as the display asks: the newest redrawn in place under a header naming
 * who sends it and how fast, or a record a line of the sender, the rate and the value.
 * --value-only leaves out the header and the sender and rate. */
class ValueStream {
public:
    ValueStream(app::Context& ctx, std::string name);

    /* One value as it came. Live, only the newest is decoded, when it is drawn. */
    void add(const std::string& from, rant::Bytes data, const rant::Schema& schema);

    /* Redraws when due, so the rate and age keep moving between values. */
    void tick();

    /* A line over the value while the view shows one, such as a task cancelling. On stderr
     * when nothing is drawn. */
    void note(const std::string& text);

    /* Draws a value not yet shown and gives the cursor back. A label, such as "result", heads
     * what is printed next when the view showed a value. */
    void finish(const std::string& label = {});

    long seen() const { return seen_; }

private:
    void draw(Rate::Clock::time_point now);
    std::string header(Rate::Clock::time_point now);    /* who sends it, how fast, how many */

    struct Latest {
        std::vector<uint8_t> data;
        rant::Schema         schema;
        std::string          from;
    };

    app::Context&                ctx_;
    std::string                  name_;
    Printer                      printer_;
    std::optional<ui::LiveView>  view_;
    std::optional<Latest>        latest_;
    std::string                  note_;
    Rate                         rate_;
    bool                         value_only_;
    long                         seen_ = 0;
    Rate::Clock::time_point      last_msg_{}, last_draw_{};
    bool                         dirty_ = true;
};

}
