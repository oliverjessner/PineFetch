"""Download a smoke-test model or run real, offline Whisper inference."""

import argparse
import errno
import json
import os
from pathlib import Path
import socket
import sys


def check_sandbox(probes_path):
    probes = json.loads(Path(probes_path).read_text(encoding="utf-8"))
    denied = []
    for directory in probes:
        try:
            os.listdir(directory)
        except OSError as error:
            if error.errno not in (errno.EPERM, errno.EACCES):
                raise RuntimeError(f"Unexpected sandbox probe error: {directory}: {error}") from error
            denied.append(directory)
        else:
            raise RuntimeError(f"Sandbox allowed reading a denied directory: {directory}")
    if not denied:
        raise RuntimeError("No existing directories were available to verify the read sandbox")
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.settimeout(2)
            probe.connect(("127.0.0.1", 9))
    except OSError as error:
        if error.errno not in (errno.EPERM, errno.EACCES):
            raise RuntimeError(f"Network denial was not verified: {error}") from error
    else:
        raise RuntimeError("Sandbox allowed an outbound network connection")
    return {"denied_directory_reads": denied, "network_connection_denied": True}


def infer(args):
    sandbox = check_sandbox(args.sandbox_probes)
    import av
    import ctranslate2
    import faster_whisper
    import numpy
    import tokenizers
    from faster_whisper import WhisperModel

    runtime = Path(sys.executable).resolve().parent.parent
    modules = (av, ctranslate2, faster_whisper, numpy, tokenizers)
    for module in modules:
        if not Path(module.__file__).resolve().is_relative_to(runtime):
            raise RuntimeError(f"Inference dependency escapes the copied runtime: {module.__name__}")
    model = WhisperModel(
        args.model,
        device="cpu",
        compute_type="int8",
        cpu_threads=2,
        num_workers=1,
        local_files_only=True,
    )
    segments, info = model.transcribe(
        args.audio,
        language="en",
        beam_size=1,
        best_of=1,
        temperature=0,
        vad_filter=False,
        condition_on_previous_text=False,
    )
    captured = [{"start": part.start, "end": part.end, "text": part.text} for part in segments]
    text = " ".join(part["text"].strip() for part in captured)
    if not captured or "local" not in text.casefold():
        raise RuntimeError(f"Expected recognizable speech about local media, received: {text!r}")
    if any(part["start"] < 0 or part["end"] < part["start"] for part in captured):
        raise RuntimeError("Inference produced invalid segment timestamps")
    return {
        "versions": {
            "python": sys.version.split()[0],
            "faster_whisper": faster_whisper.__version__,
            "av": av.__version__,
            "ctranslate2": ctranslate2.__version__,
        },
        "device": "cpu",
        "compute_type": "int8",
        "local_files_only": True,
        "language": info.language,
        "duration_seconds": info.duration,
        "segment_count": len(captured),
        "text": text,
        "segments": captured,
        "sandbox": sandbox,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    download = commands.add_parser("download", help="Download tiny into the explicitly isolated cache")
    download.add_argument("cache")
    inference = commands.add_parser("infer", help="Run offline CPU/int8 inference and verify the sandbox")
    inference.add_argument("--model", required=True)
    inference.add_argument("--audio", required=True)
    inference.add_argument("--sandbox-probes", required=True)
    args = parser.parse_args()
    if args.command == "download":
        from faster_whisper.utils import download_model

        result = {"model": "tiny", "model_path": download_model("tiny", cache_dir=args.cache)}
    else:
        result = infer(args)
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
