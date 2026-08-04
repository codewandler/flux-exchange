#!/usr/bin/env python3
"""Validate the sole GitHub release-asset redirect before curl follows it."""

import re
import sys
import urllib.parse

ALLOWED = set(
    "jwt response-content-disposition response-content-type rscd rsct se sig ske skoid sks skt sktid skv sp spr sr sv".split()
)
PATH = re.compile(
    r"/github-production-release-asset/[1-9][0-9]{0,19}/"
    r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
)
# RFC 3986 query-value characters, excluding `&` because it is the pair separator. Rejecting
# everything else here keeps malformed raw URI bytes from reaching curl even when percent escapes
# themselves happen to be well formed.
VALUE = re.compile(r"(?:[A-Za-z0-9._~!$'()*+,;=:@/?-]|%[0-9A-Fa-f]{2})*")


def validate(raw: str) -> None:
    try:
        encoded = raw.encode("ascii", "strict")
    except UnicodeEncodeError as error:
        raise ValueError("redirect is not ASCII") from error
    if len(encoded) > 8192:
        raise ValueError("redirect is too long")
    try:
        url = urllib.parse.urlsplit(raw)
        port = url.port
    except ValueError as error:
        raise ValueError("redirect authority is malformed") from error
    if (
        url.scheme != "https"
        or url.hostname != "release-assets.githubusercontent.com"
        or url.netloc not in ("release-assets.githubusercontent.com", "release-assets.githubusercontent.com:443")
        or port not in (None, 443)
        or url.username
        or url.password
        or url.fragment
    ):
        raise ValueError("redirect authority is outside policy")
    if not PATH.fullmatch(url.path):
        raise ValueError("redirect path is outside policy")
    if not url.query or len(url.query.encode("ascii")) > 6144:
        raise ValueError("redirect query is absent or too long")
    seen = set()
    for part in url.query.split("&"):
        if "=" not in part:
            raise ValueError("redirect query value is absent")
        name, value = part.split("=", 1)
        if (
            not name
            or "%" in name
            or name not in ALLOWED
            or name in seen
            or not value
            or len(value.encode("ascii")) > 2048
            or not VALUE.fullmatch(value)
        ):
            raise ValueError("redirect query is outside policy")
        seen.add(name)
        if any(byte < 0x20 or byte == 0x7F for byte in urllib.parse.unquote_to_bytes(value)):
            raise ValueError("redirect query decodes to a control byte")


def self_test() -> None:
    base = "https://release-assets.githubusercontent.com/github-production-release-asset/1/00000000-0000-0000-0000-000000000001?sig=good%20value"
    validate(base)
    rejected = [
        base.replace("https://", "http://"),
        base.replace("release-assets.githubusercontent.com", "example.com"),
        base.replace("release-assets.githubusercontent.com", "RELEASE-ASSETS.GITHUBUSERCONTENT.COM"),
        base.replace("?sig=", "?unknown="),
        base.replace("good%20value", "%ZZ"),
        base.replace("good%20value", "%0a"),
        base.replace("good%20value", "raw value"),
        base.replace("good%20value", "raw[value"),
        base + "&sig=again",
        base + "#fragment",
    ]
    for candidate in rejected:
        try:
            validate(candidate)
        except ValueError:
            continue
        raise SystemExit(f"self-test accepted inadmissible redirect: {candidate}")
    print("PASS release redirect validator self-test")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    elif len(sys.argv) == 2:
        try:
            validate(sys.argv[1])
        except ValueError as error:
            raise SystemExit(f"release-download: {error}") from error
    else:
        raise SystemExit("usage: release-validate-redirect.py <url>|--self-test")
