import pytest
from burner_redis import BurnerRedis


@pytest.fixture
def r():
    """Create a fresh BurnerRedis instance for each test."""
    return BurnerRedis()
