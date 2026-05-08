from pathlib import Path

from httpx import AsyncClient


async def test_system_settings_default_and_update(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    r = await client.get("/api/v1/settings/system", headers=auth_headers)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["group_workspace_root"] is None
    assert body["web_search_provider"] == "tavily"
    assert body["tavily_api_key_configured"] is False
    assert body["tavily_search_url"] == "https://api.tavily.com/search"
    assert body["tavily_max_results"] == 5
    assert body["tavily_search_depth"] == "basic"
    assert body["tavily_include_answer"] is True
    assert body["tavily_include_raw_content"] is False

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


async def test_system_settings_updates_tavily_web_search(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.patch(
        "/api/v1/settings/system",
        headers=auth_headers,
        json={
            "web_search_provider": "tavily",
            "tavily_api_key": " tvly-test ",
            "tavily_search_url": "https://tavily.internal/search",
            "tavily_max_results": 8,
            "tavily_search_depth": "advanced",
            "tavily_include_answer": False,
            "tavily_include_raw_content": True,
        },
    )

    assert r.status_code == 200, r.text
    body = r.json()
    assert body["web_search_provider"] == "tavily"
    assert body["tavily_api_key_configured"] is True
    assert "tavily_api_key" not in body
    assert body["tavily_search_url"] == "https://tavily.internal/search"
    assert body["tavily_max_results"] == 8
    assert body["tavily_search_depth"] == "advanced"
    assert body["tavily_include_answer"] is False
    assert body["tavily_include_raw_content"] is True

    r = await client.patch(
        "/api/v1/settings/system",
        headers=auth_headers,
        json={"tavily_api_key": None, "tavily_search_url": ""},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["tavily_api_key_configured"] is False
    assert body["tavily_search_url"] == "https://api.tavily.com/search"


async def test_system_settings_rejects_invalid_tavily_values(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.patch(
        "/api/v1/settings/system",
        headers=auth_headers,
        json={"tavily_search_url": "ftp://example.test/search"},
    )
    assert r.status_code == 400, r.text
    assert "http or https" in r.json()["error"]["message"]

    r = await client.patch(
        "/api/v1/settings/system",
        headers=auth_headers,
        json={"tavily_search_depth": "deep"},
    )
    assert r.status_code == 400, r.text
    assert "basic or advanced" in r.json()["error"]["message"]
