"""Tests of make_voice.py: what the support contact's recordings say and how the two languages
are put together. Run: python3 -m unittest make_voice_test (from this folder; needs numpy)."""

import unittest

import numpy as np

import make_voice


class Texts(unittest.TestCase):
    def test_both_recordings_speak_english_first_then_spanish(self):
        for recording in (make_voice.CALL, make_voice.VOICE_MESSAGE):
            self.assertEqual(list(recording), ["en", "es"])

    def test_the_brand_voice_is_sober(self):
        for recording in (make_voice.CALL, make_voice.VOICE_MESSAGE):
            for text in recording.values():
                self.assertNotIn("!", text)
                self.assertNotIn("¡", text)

    def test_the_call_says_it_is_a_recording_that_hangs_up_by_itself(self):
        self.assertIn("recording", make_voice.CALL["en"])
        self.assertIn("hangs up by itself", make_voice.CALL["en"])
        self.assertIn("grabación", make_voice.CALL["es"])

    def test_the_voices_are_the_manual_s(self):
        self.assertEqual(make_voice.VOICES["en"], ("af_heart", 0.97, "en-us"))
        self.assertEqual(make_voice.VOICES["es"], ("ef_dora", 1.0, "es"))


class Spoken(unittest.TestCase):
    def test_the_name_is_spelt_as_each_language_says_it(self):
        self.assertEqual(make_voice.spoken("Thanks for trying FlickerTalk.", "en"), "Thanks for trying Flicker Talk.")
        self.assertEqual(make_voice.spoken("This is FlickerTalk's support.", "en"), "This is Flicker Talk's support.")
        self.assertEqual(make_voice.spoken("Gracias por probar FlickerTalk.", "es"), "Gracias por probar Flíker Tok.")

    def test_other_words_are_left_alone(self):
        self.assertEqual(make_voice.spoken("Your voice message arrived.", "en"), "Your voice message arrived.")


class Join(unittest.TestCase):
    def test_the_languages_are_joined_with_a_pause_between_them(self):
        english, spanish = np.ones(3, dtype=np.float32), np.full(2, 0.5, dtype=np.float32)
        joined = make_voice.join([english, spanish], rate=10, pause_s=0.4)
        self.assertEqual(joined.tolist(), [1, 1, 1, 0, 0, 0, 0, 0.5, 0.5])

    def test_one_part_needs_no_pause(self):
        self.assertEqual(make_voice.join([np.ones(2, dtype=np.float32)], rate=10, pause_s=1).tolist(), [1, 1])


if __name__ == "__main__":
    unittest.main()
