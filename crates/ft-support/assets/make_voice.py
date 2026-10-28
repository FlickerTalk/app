"""Speaks the support contact's two recordings with Kokoro (local neural TTS), in the voices of
the user manual's narration: English first, then Spanish, one mono WAV each.

    python make_voice.py <folder>     # writes call.wav and voice.wav there

Needs kokoro_onnx, numpy and soundfile, and the Kokoro model and voices: FT_KOKORO_MODEL and
FT_KOKORO_VOICES (by default the umbrella's video/assets, next to the app's checkout).
make-assets.sh calls it and encodes the WAVs.
"""

import os
import re
import sys
from pathlib import Path

import numpy as np

# The manual's narration voices (name, speed, the language Kokoro phonemizes with).
VOICES = {"en": ("af_heart", 0.97, "en-us"), "es": ("ef_dora", 1.0, "es")}

# What a call hears, on a loop.
CALL = {
    "en": "Hi, this is FlickerTalk's automated support contact. You've reached a recording, so your call works: "
    "audio and video are flowing end to end. Try sending a message, a file or a voice message, and they get "
    "answered. This call hangs up by itself in a couple of minutes. Thanks for trying FlickerTalk.",
    "es": "Hola, este es el contacto de soporte automático de FlickerTalk. Has llegado a una grabación, así que tu "
    "llamada funciona: el audio y el vídeo llegan de extremo a extremo. Prueba a enviar un mensaje, un archivo o "
    "un mensaje de voz, y recibirás respuesta. Esta llamada se cuelga sola en un par de minutos. Gracias por "
    "probar FlickerTalk.",
}

# The voice message sent back for a voice message.
VOICE_MESSAGE = {
    "en": "Hi, this is FlickerTalk support. Your voice message arrived. This is an automatic reply so you can hear "
    "that voice messages work.",
    "es": "Hola, soy el soporte de FlickerTalk. Tu mensaje de voz ha llegado. Esta es una respuesta automática para "
    "que compruebes que los mensajes de voz funcionan.",
}

# Silence between the two languages.
PAUSE_S = 0.7


# How each language says the app's name (as the manual's narration does, video/manual/kit/tts.py).
NAME = {"en": "Flicker Talk", "es": "Flíker Tok"}


def spoken(text: str, lang: str) -> str:
    """The text as Kokoro should say it: the app's name as each language pronounces it."""
    return re.sub(r"\bFlickerTalk\b", NAME[lang], text)


def join(parts: list, rate: int, pause_s: float) -> np.ndarray:
    """The parts one after the other, with `pause_s` of silence between two of them."""
    pause = np.zeros(int(round(rate * pause_s)), dtype=np.float32)
    pieces = []
    for index, part in enumerate(parts):
        if index:
            pieces.append(pause)
        pieces.append(np.asarray(part, dtype=np.float32))
    return np.concatenate(pieces)


def say(kokoro, texts: dict) -> tuple:
    """Both languages of one recording, English first: (samples, rate)."""
    parts, rate = [], None
    for lang in ("en", "es"):
        name, speed, phonemes = VOICES[lang]
        samples, rate = kokoro.create(spoken(texts[lang], lang), voice=name, speed=speed, lang=phonemes)
        parts.append(samples)
    return join(parts, rate, PAUSE_S), rate


def main(folder: str) -> None:
    import soundfile
    from kokoro_onnx import Kokoro

    assets = Path(__file__).resolve().parent
    # The umbrella's video/assets, next to the app's checkout, unless told otherwise.
    shared = assets.parents[3] / "video" / "assets"
    model = os.environ.get("FT_KOKORO_MODEL", str(shared / "kokoro-v1.0.onnx"))
    voices = os.environ.get("FT_KOKORO_VOICES", str(shared / "voices-v1.0.bin"))
    kokoro = Kokoro(model, voices)
    out = Path(folder)
    for name, texts in (("call.wav", CALL), ("voice.wav", VOICE_MESSAGE)):
        samples, rate = say(kokoro, texts)
        soundfile.write(out / name, samples, rate)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else ".")
