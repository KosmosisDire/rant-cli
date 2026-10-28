#include "commands/stream.hpp"

#include <cmath>
#include <cstdio>

#include "mesh/values.hpp"

namespace commands {

using mesh::json;
using Clock = Rate::Clock;

double Rate::hz(Clock::time_point now) {
    while (!times_.empty() && now - times_.front() > std::chrono::seconds(2)) times_.pop_front();
    if (times_.size() < 2) return 0;
    double span = std::chrono::duration<double>(times_.back() - times_.front()).count();
    return span > 0 ? (times_.size() - 1) / span : 0;
}

static std::string fixed(double v, int decimals) {
    char buf[32];
    std::snprintf(buf, sizeof buf, "%.*f", decimals, v);
    return buf;
}

ValueStream::ValueStream(app::Context& ctx, std::string name)
    : ctx_(ctx), name_(std::move(name)), printer_(format_of(ctx), &ctx.out, true) {
    if (live_of(ctx)) view_.emplace();
}

void ValueStream::add(const std::string& from, rant::Bytes data, const rant::Schema& schema) {
    seen_++;
    last_msg_ = Clock::now();
    rate_.hit(last_msg_);
    if (!view_) {
        ctx_.out.line(printer_.line(json{ { "from", from },
                                          { "hz", std::round(rate_.hz(last_msg_) * 10) / 10 },
                                          { "value", mesh::to_json(data, schema) } }));
        std::fflush(stdout);
        return;
    }
    latest_ = Latest{ { data.data(), data.data() + data.size() }, schema, from };
    dirty_ = true;
    tick();
}

/* A new value redraws at 30 frames a second at most, and the rate and age keep moving
 * between values at 4. */
void ValueStream::tick() {
    if (!view_) return;
    auto now = Clock::now();
    if ((dirty_ && now - last_draw_ >= std::chrono::milliseconds(33)) || now - last_draw_ >= std::chrono::milliseconds(250))
        draw(now);
}

void ValueStream::finish() {
    if (!view_) return;
    if (dirty_ && latest_) draw(Clock::now());
    view_.reset();
}

void ValueStream::draw(Clock::time_point now) {
    std::string status;
    if (!latest_) {
        status = "waiting for a message";
    } else {
        double age = std::chrono::duration<double>(now - last_msg_).count();
        std::string hz = fixed(rate_.hz(now), 1);
        hz.insert(0, hz.size() < 6 ? 6 - hz.size() : 0, ' ');    /* the header holds still too */
        status = "from " + latest_->from + "  " + hz + " Hz  " + std::to_string(seen_) +
                 (seen_ == 1 ? " message" : " messages");
        if (age > 1) status += "  last " + fixed(age, 1) + " s ago";
    }
    std::vector<std::string> lines{ ctx_.out.paint(ui::Style::Bold, name_) + "  " + ctx_.out.paint(ui::Style::Faint, status) };
    if (latest_) {
        std::string body = printer_.block(mesh::to_json(rant::Bytes(latest_->data.data(), latest_->data.size()), latest_->schema));
        size_t start = 0;
        while (start <= body.size()) {
            size_t end = body.find('\n', start);
            if (end == std::string::npos) end = body.size();
            lines.push_back(body.substr(start, end - start));
            start = end + 1;
        }
    }
    view_->draw(lines);
    last_draw_ = now;
    dirty_ = false;
}

}
