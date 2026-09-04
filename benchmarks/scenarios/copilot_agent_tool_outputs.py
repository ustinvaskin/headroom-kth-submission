"""Exact, compact representations of JSON tool output from Copilot CLI tasks."""

from __future__ import annotations

import hashlib
import json


CAPTURED_TASKS = (
    {
        "name": "median.json",
        "status": "even-length-regression",
        "value": (
            "Median must average the two middle values for an even-sized input; "
            "verify the lower-middle index."
        ),
        "input_sha256": "ea724acd911a671313c1f0d9f45262b7507d1b1e38abfabbfefd23a34f444181",
    },
    {
        "name": "currency.json",
        "status": "negative-currency-regression",
        "value": (
            "Negative cents must use an absolute magnitude before splitting dollars "
            "and remainder; keep the sign before the currency symbol."
        ),
        "input_sha256": "af5119c47eb977f78fdfbcb19a3817214c68255235773b8cc42172cac3a99b38",
    },
    {
        "name": "slug.json",
        "status": "slug-normalization-regression",
        "value": (
            "A slug keeps lowercase alphanumeric words, removes punctuation, and "
            "collapses consecutive separators to one hyphen."
        ),
        "input_sha256": "911d3f45d8790a48f678eccb0e437634494c5ad98f2d05b44d476001710c6ebd",
    },
)


def captured_payloads() -> list[tuple[str, str]]:
    """Return exact JSON arrays captured from three successful Copilot tasks."""
    payloads = []
    for task in CAPTURED_TASKS:
        payload = json.dumps(
            [
                {"id": index, "status": task["status"], "value": task["value"]}
                for index in range(100)
            ]
        )
        observed_hash = hashlib.sha256(payload.encode("utf-8")).hexdigest()
        if observed_hash != task["input_sha256"]:
            raise RuntimeError(f"captured payload hash changed for {task['name']}")
        payloads.append((task["name"], payload))
    return payloads