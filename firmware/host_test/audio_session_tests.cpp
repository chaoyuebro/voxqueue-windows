#include <cassert>
#include <string>

#include "keyboard/audio_session.h"

int main() {
  ai_keyboard::AudioSessionLifecycle lifecycle;
  assert(!lifecycle.active());
  assert(lifecycle.request_start(0) == ai_keyboard::AudioSessionStartResult::Rejected);

  assert(lifecycle.request_start(42) == ai_keyboard::AudioSessionStartResult::Started);
  const auto first_generation = lifecycle.generation();
  assert(first_generation != 0);
  assert(lifecycle.should_run(first_generation));
  assert(!lifecycle.clean_stop_requested(first_generation));
  assert(lifecycle.mark_streaming(first_generation));
  assert(lifecycle.phase() == ai_keyboard::AudioSessionPhase::Streaming);
  assert(lifecycle.request_start(42) == ai_keyboard::AudioSessionStartResult::AlreadyActive);
  assert(lifecycle.request_start(43) == ai_keyboard::AudioSessionStartResult::NeedsStop);

  assert(!lifecycle.request_stop(43, "stale_stop"));
  assert(lifecycle.request_stop(42, "client_stop"));
  assert(lifecycle.clean_stop_requested(first_generation));
  assert(!lifecycle.clean_stop_requested(first_generation + 1));
  assert(!lifecycle.should_run(first_generation));
  assert(lifecycle.finish(first_generation, "stream_stop"));
  assert(!lifecycle.active());
  assert(lifecycle.stop_reason() == "client_stop");

  assert(lifecycle.request_start(43) == ai_keyboard::AudioSessionStartResult::Started);
  const auto second_generation = lifecycle.generation();
  assert(second_generation != first_generation);
  assert(!lifecycle.mark_recovering(first_generation));
  assert(!lifecycle.finish(first_generation, "stale_cleanup"));
  assert(lifecycle.session_id() == 43);
  assert(lifecycle.mark_recovering(second_generation));
  assert(lifecycle.phase() == ai_keyboard::AudioSessionPhase::Recovering);
  assert(lifecycle.mark_streaming(second_generation));
  assert(lifecycle.finish(second_generation, "udp_recovery_exhausted"));
  assert(lifecycle.stop_reason() == "udp_recovery_exhausted");

  // The sender samples running, then a release occurs during its queue wait.
  assert(lifecycle.request_start(44) == ai_keyboard::AudioSessionStartResult::Started);
  const auto third_generation = lifecycle.generation();
  const bool sampled_running = lifecycle.should_run(third_generation);
  assert(sampled_running);
  assert(lifecycle.request_stop(44, "client_stop"));
  assert(lifecycle.clean_stop_requested(third_generation));
  assert(lifecycle.finish(third_generation, "client_stop"));
  assert(!lifecycle.clean_stop_requested(third_generation));
  assert(lifecycle.request_start(45) == ai_keyboard::AudioSessionStartResult::Started);
  assert(lifecycle.request_stop(45, "i2s_recovery_exhausted"));
  assert(!lifecycle.clean_stop_requested(lifecycle.generation()));
  assert(lifecycle.finish(lifecycle.generation(), "i2s_recovery_exhausted"));
  assert(lifecycle.request_start(46) == ai_keyboard::AudioSessionStartResult::Started);
  assert(lifecycle.request_stop(46, "max_duration"));
  assert(lifecycle.clean_stop_requested(lifecycle.generation()));

  return 0;
}
