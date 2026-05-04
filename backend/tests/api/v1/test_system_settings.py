from pathlib import Path

from httpx import AsyncClient


async def test_system_settings_default_and_update(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    r = await client.get("/api/v1/settings/system", headers=auth_headers)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["group_workspace_root"] is None

    r = await client.patch(
        "/api/v1/settings/system",
        headers=auth_headers,
        json={"group_workspace_root": str(tmp_path)},
    )
    assert r.status_code == 200, r.text
    assert Path(r.json()["group_workspace_root"]) == tmp_path.resolve()

    # Subsequent GET reflects the saved value.
    r = await client.get("/api/v1/settings/system", headers=auth_headers)
    assert Path(r.json()["group_workspace_root"]) == tmp_path.resolve()


async def test_system_settings_rejects_missing_directory(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    missing = tmp_path / "does-not-exist"
    r = await client.patch(
        "/api/v1/settings/system",
        headers=auth_headers,
        json={"group_workspace_root": str(missing)},
    )
    assert r.status_code == 400, r.text
    assert "existing directory" in r.json()["error"]["message"].lower()


async def test_system_settings_can_be_cleared(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    await client.patch(
        "/api/v1/settings/system",
        headers=auth_headers,
        json={"group_workspace_root": str(tmp_path)},
    )
    r = await client.patch(
        "/api/v1/settings/system",
        headers=auth_headers,
        json={"group_workspace_root": None},
    )
    assert r.status_code == 200, r.text
    assert r.json()["group_workspace_root"] is None
