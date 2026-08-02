#!/usr/bin/env python3
"""Build a synthetic OAuth callback for the e2e harness.

Every token here is fabricated. The JWT is unsigned nonsense with a recognisable
signature string, the account is `wl-e2e@example.invalid` (a reserved TLD that
can never resolve), and the first name is `Testinald` — chosen because it cannot
collide with a real dictionary entry, which is what makes the seeded row a
reliable signal that `publish_session` ran.

Nothing produced here is ever written to the user's own keychain: the harness
runs the app under a throwaway HOME with its own login keychain.

    fake_callback.py            -> a wisprlightning://auth/google/success URL
    fake_callback.py --file     -> the Supabase-shaped session.json body
    fake_callback.py --file --alt  -> the same, with a different token, so a
                                      second write is distinguishable
"""

import base64
import json
import sys
import time
import urllib.parse

ACCESS_PREFIX = "wl-e2e-fake-access"
REFRESH_PREFIX = "wl-e2e-fake-refresh"
EMAIL = "wl-e2e@example.invalid"
FIRST = "Testinald"
LAST = "Fixture"


def b64(payload: bytes) -> str:
    return base64.urlsafe_b64encode(payload).decode().rstrip("=")


def jwt(suffix: str) -> str:
    """An unsigned JWT the session parser can read the identity out of.

    The app only decodes the payload; it never verifies a signature, so a
    fabricated third segment is enough and is deliberately not a valid one.
    """
    header = b64(json.dumps({"alg": "HS256", "typ": "JWT"}).encode())
    body = b64(
        json.dumps(
            {
                "sub": "00000000-0000-4000-8000-00000000e2e5",
                "email": EMAIL,
                "exp": int(time.time()) + 3600,
                "user_metadata": {
                    "full_name": f"{FIRST} {LAST}",
                    "avatar_url": "https://example.invalid/avatar.png",
                },
            }
        ).encode()
    )
    return f"{header}.{body}.{ACCESS_PREFIX}-{suffix}"


def main() -> None:
    alt = "--alt" in sys.argv
    suffix = "alt" if alt else "primary"
    access = jwt(suffix)
    refresh = f"{REFRESH_PREFIX}-{suffix}"
    expires_at = int(time.time()) + 3600

    if "--file" in sys.argv:
        # The shape the commercial app writes: one `sb-<ref>-auth-token` key
        # whose value is a JSON string holding the session.
        inner = json.dumps(
            {
                "access_token": access,
                "refresh_token": refresh,
                "expires_at": expires_at,
                "user": {
                    "id": "00000000-0000-4000-8000-00000000e2e5",
                    "email": EMAIL,
                    "user_metadata": {
                        "full_name": f"{FIRST} {LAST}",
                        "avatar_url": "https://example.invalid/avatar.png",
                    },
                },
            }
        )
        print(json.dumps({"sb-e2efixture-auth-token": inner}))
        return

    query = urllib.parse.urlencode(
        {
            "access_token": access,
            "refresh_token": refresh,
            "expires_at": expires_at,
            "first_name": FIRST,
            "last_name": LAST,
        }
    )
    print(f"wisprlightning://auth/google/success?{query}")


if __name__ == "__main__":
    main()
