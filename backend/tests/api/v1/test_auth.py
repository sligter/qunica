import secrets

from httpx import AsyncClient


async def test_register_creates_user(client: AsyncClient) -> None:
    suffix = secrets.token_hex(4)
    r = await client.post(
        "/api/v1/auth/register",
        json={
            "email": f"u-{suffix}@example.com",
            "password": "valid-password-1",
            "name": "U",
        },
    )
    assert r.status_code == 201
    body = r.json()
    assert body["email"] == f"u-{suffix}@example.com"
    assert body["name"] == "U"
    assert "id" in body and "password" not in body


async def test_register_duplicate_email_conflicts(client: AsyncClient) -> None:
    payload = {
        "email": "dup@example.com",
        "password": "valid-password-1",
        "name": "Dup",
    }
    r = await client.post("/api/v1/auth/register", json=payload)
    assert r.status_code == 201
    r = await client.post("/api/v1/auth/register", json=payload)
    assert r.status_code == 409
    assert r.json()["error"]["code"] == "conflict"


async def test_login_success_returns_jwt(client: AsyncClient) -> None:
    suffix = secrets.token_hex(4)
    email = f"login-{suffix}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email, "password": "valid-password-1", "name": "L"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email, "password": "valid-password-1"},
    )
    assert r.status_code == 200
    assert r.json()["token_type"] == "bearer"
    assert r.json()["access_token"].count(".") == 2


async def test_login_bad_password_denied(client: AsyncClient) -> None:
    suffix = secrets.token_hex(4)
    email = f"bad-{suffix}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email, "password": "valid-password-1", "name": "B"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email, "password": "wrong-password-2"},
    )
    assert r.status_code == 403
    assert r.json()["error"]["code"] == "permission_denied"


async def test_me_requires_bearer(client: AsyncClient) -> None:
    r = await client.get("/api/v1/auth/me")
    assert r.status_code == 401


async def test_me_returns_user_for_valid_token(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.get("/api/v1/auth/me", headers=auth_headers)
    assert r.status_code == 200
    assert r.json()["email"].startswith("test-")
