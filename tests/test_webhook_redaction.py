import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from features import webhook

CONFIG = {"enabled": True, "url": "https://example.invalid/hook", "log_info": True}


def make_interceptor(config=CONFIG):
    return webhook.WebhookStdoutInterceptor(None, lambda: config)


class WebhookRedactionTests(unittest.TestCase):
    def test_link_code_is_masked_before_it_is_queued(self):
        interceptor = make_interceptor()
        with mock.patch.object(interceptor, "_enqueue") as enqueue:
            interceptor.write("[INFO] Private server (link code: SECRET123)\n")
        line = enqueue.call_args.args[1]
        self.assertNotIn("SECRET123", line)
        self.assertIn("[REDACTED]", line)

    def test_vip_url_is_masked(self):
        interceptor = make_interceptor()
        with mock.patch.object(interceptor, "_enqueue") as enqueue:
            interceptor.write("[INFO] join_vip_server: alice -> https://www.roblox.com/games/1/x?privateServerLinkCode=SECRET123\n")
        self.assertNotIn("SECRET123", enqueue.call_args.args[1])

    def test_auto_rejoin_embed_is_masked(self):
        interceptor = make_interceptor({**CONFIG, "log_auto_rejoin": True})
        with mock.patch.object(interceptor, "_send_ar_embed") as embed:
            interceptor.write("[Auto-Rejoin] [alice] rejoining privateServerLinkCode=SECRET123\n")
        self.assertNotIn("SECRET123", embed.call_args.args[2])

    def test_console_pane_keeps_the_original_text(self):
        interceptor = make_interceptor({})
        interceptor.write("[INFO] Private server (link code: SECRET123)\n")
        self.assertIn("SECRET123", interceptor._console_queue[0][0])

    def test_ordinary_lines_are_unchanged(self):
        interceptor = make_interceptor()
        with mock.patch.object(interceptor, "_enqueue") as enqueue:
            interceptor.write("[INFO] Launching Roblox for alice...\n")
        self.assertEqual(enqueue.call_args.args[1], "[INFO] Launching Roblox for alice...")


if __name__ == "__main__":
    unittest.main()
