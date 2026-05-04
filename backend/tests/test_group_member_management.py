from httpx import AsyncClient
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.security import hash_password
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.message import Message
from app.models.user import User


async def _create_user(db: AsyncSession, email: str, name: str) -> User:
    user = User(email=email, name=name, password_hash=hash_password("test-password-123"))
    db.add(user)
    await db.flush()
    await db.refresh(user)
    return user


async def _create_group_with_owner(db: AsyncSession, owner: User) -> Group:
    group = Group(owner_id=owner.id, name="Test group")
    db.add(group)
    await db.flush()
    db.add(GroupMember(group_id=group.id, user_id=owner.id, role="owner"))
    await db.flush()
    await db.refresh(group)
    return group


async def test_owner_can_add_mute_unmute_and_remove_human_member(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    owner_id = me.json()["id"]
    owner = await db_session.scalar(select(User).where(User.id == owner_id))
    assert owner is not None
    target = await _create_user(db_session, "member@example.com", "Member User")
    group = await _create_group_with_owner(db_session, owner)

    add_response = await client.post(
        f"/api/v1/groups/{group.id}/members",
        headers=auth_headers,
        json={"user_id": str(target.id)},
    )
    assert add_response.status_code == 201, add_response.text
    assert add_response.json()["user_id"] == str(target.id)

    mute_response = await client.patch(
        f"/api/v1/groups/{group.id}/members/{target.id}/mute",
        headers=auth_headers,
        json={"muted": True},
    )
    assert mute_response.status_code == 200, mute_response.text
    assert mute_response.json()["is_muted"] is True
    await db_session.refresh(group)
    assert str(target.id) in (group.muted_member_ids or [])
    assert str(target.id) not in (group.admin_agent_ids or [])

    unmute_response = await client.patch(
        f"/api/v1/groups/{group.id}/members/{target.id}/mute",
        headers=auth_headers,
        json={"muted": False},
    )
    assert unmute_response.status_code == 200, unmute_response.text
    assert unmute_response.json()["is_muted"] is False
    await db_session.refresh(group)
    assert str(target.id) not in (group.muted_member_ids or [])

    remove_response = await client.delete(
        f"/api/v1/groups/{group.id}/members/{target.id}",
        headers=auth_headers,
    )
    assert remove_response.status_code == 204, remove_response.text
    membership = await db_session.scalar(
        select(GroupMember).where(
            GroupMember.group_id == group.id,
            GroupMember.user_id == target.id,
        )
    )
    assert membership is not None
    assert membership.status == "removed"


async def test_muted_agent_does_not_reply_to_mentions(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    owner_id = me.json()["id"]
    owner = await db_session.scalar(select(User).where(User.id == owner_id))
    assert owner is not None
    group = await _create_group_with_owner(db_session, owner)
    agent = Agent(owner_id=owner.id, name="Helper", system_prompt="Help")
    db_session.add(agent)
    await db_session.flush()
    db_session.add(GroupAgent(group_id=group.id, agent_id=agent.id))
    group.muted_agent_ids = [str(agent.id)]
    await db_session.flush()

    response = await client.post(
        f"/api/v1/groups/{group.id}/messages",
        headers=auth_headers,
        json={"content": "@Helper hello"},
    )

    assert response.status_code == 201, response.text
    assert response.json()["agent_replies"] == []
    agent_message = await db_session.scalar(
        select(Message).where(Message.group_id == group.id, Message.sender_type == "agent")
    )
    assert agent_message is None


async def test_owner_can_mute_unmute_and_remove_agent_member(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    owner_id = me.json()["id"]
    owner = await db_session.scalar(select(User).where(User.id == owner_id))
    assert owner is not None
    group = await _create_group_with_owner(db_session, owner)
    agent = Agent(owner_id=owner.id, name="Helper", system_prompt="Help")
    db_session.add(agent)
    await db_session.flush()
    db_session.add(GroupAgent(group_id=group.id, agent_id=agent.id))
    await db_session.flush()

    mute_response = await client.patch(
        f"/api/v1/groups/{group.id}/agents/{agent.id}/mute",
        headers=auth_headers,
        json={"muted": True},
    )
    assert mute_response.status_code == 200, mute_response.text
    await db_session.refresh(group)
    assert str(agent.id) in (group.muted_agent_ids or [])

    unmute_response = await client.patch(
        f"/api/v1/groups/{group.id}/agents/{agent.id}/mute",
        headers=auth_headers,
        json={"muted": False},
    )
    assert unmute_response.status_code == 200, unmute_response.text
    await db_session.refresh(group)
    assert str(agent.id) not in (group.muted_agent_ids or [])

    remove_response = await client.delete(
        f"/api/v1/groups/{group.id}/agents/{agent.id}",
        headers=auth_headers,
    )
    assert remove_response.status_code == 204, remove_response.text
    group_agent = await db_session.scalar(
        select(GroupAgent).where(
            GroupAgent.group_id == group.id,
            GroupAgent.agent_id == agent.id,
        )
    )
    assert group_agent is not None
    assert group_agent.status == "removed"


async def test_non_owner_cannot_mutate_members_or_agents(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    owner_id = me.json()["id"]
    owner = await db_session.scalar(select(User).where(User.id == owner_id))
    assert owner is not None
    group = await _create_group_with_owner(db_session, owner)
    non_owner = await _create_user(db_session, "plain@example.com", "Plain Member")
    target = await _create_user(db_session, "target@example.com", "Target Member")
    agent = Agent(owner_id=owner.id, name="Helper", system_prompt="Help")
    db_session.add(agent)
    await db_session.flush()
    db_session.add_all(
        [
            GroupMember(group_id=group.id, user_id=non_owner.id, role="member"),
            GroupMember(group_id=group.id, user_id=target.id, role="member"),
            GroupAgent(group_id=group.id, agent_id=agent.id),
        ]
    )
    await db_session.flush()
    login = await client.post(
        "/api/v1/auth/login",
        json={"email": non_owner.email, "password": "test-password-123"},
    )
    assert login.status_code == 200, login.text
    non_owner_headers = {"Authorization": f"Bearer {login.json()['access_token']}"}

    requests = [
        client.post(
            f"/api/v1/groups/{group.id}/members",
            headers=non_owner_headers,
            json={"user_id": str(target.id)},
        ),
        client.patch(
            f"/api/v1/groups/{group.id}/members/{target.id}/mute",
            headers=non_owner_headers,
            json={"muted": True},
        ),
        client.delete(
            f"/api/v1/groups/{group.id}/members/{target.id}",
            headers=non_owner_headers,
        ),
        client.post(
            f"/api/v1/groups/{group.id}/agents",
            headers=non_owner_headers,
            json={"agent_id": str(agent.id)},
        ),
        client.patch(
            f"/api/v1/groups/{group.id}/agents/{agent.id}/mute",
            headers=non_owner_headers,
            json={"muted": True},
        ),
        client.delete(
            f"/api/v1/groups/{group.id}/agents/{agent.id}",
            headers=non_owner_headers,
        ),
    ]

    for request in requests:
        response = await request
        assert response.status_code == 403, response.text


async def test_owner_cannot_remove_or_mute_self(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    owner_id = me.json()["id"]
    owner = await db_session.scalar(select(User).where(User.id == owner_id))
    assert owner is not None
    group = await _create_group_with_owner(db_session, owner)

    remove_response = await client.delete(
        f"/api/v1/groups/{group.id}/members/{owner.id}",
        headers=auth_headers,
    )
    assert remove_response.status_code == 403, remove_response.text

    mute_response = await client.patch(
        f"/api/v1/groups/{group.id}/members/{owner.id}/mute",
        headers=auth_headers,
        json={"muted": True},
    )
    assert mute_response.status_code == 403, mute_response.text
