"""Integration tests against live WiZ bulb (192.168.0.102).

Run with: pytest --live
"""

import asyncio
import pytest
from pywizlight import PilotBuilder, wizlight

from wizctl.bulb import get_status_info
from wizctl.cli import async_main

BULB_IP = "192.168.0.102"


@pytest.mark.live
@pytest.mark.asyncio
async def test_live_bulb_full_cycle():
    """Test full cycle of operations against the physical bulb and restore initial state."""
    # 1. Capture initial state
    initial_info = await get_status_info(BULB_IP)
    assert initial_info["ip"] == BULB_IP
    assert initial_info["mac"] is not None

    try:
        # 2. Test status CLI command
        exit_code = await async_main(["--ip", BULB_IP, "status"])
        assert exit_code == 0

        # 3. Test brightness command
        exit_code = await async_main(["--ip", BULB_IP, "brightness", "80%"])
        assert exit_code == 0

        # 4. Test color command
        exit_code = await async_main(["--ip", BULB_IP, "color", "blue"])
        assert exit_code == 0

        # 5. Test kelvin command
        exit_code = await async_main(["--ip", BULB_IP, "kelvin", "3000"])
        assert exit_code == 0

        # 6. Test scene command
        exit_code = await async_main(["--ip", BULB_IP, "scene", "cozy"])
        assert exit_code == 0

    finally:
        # 7. Restore original state
        bulb = wizlight(BULB_IP)
        try:
            if initial_info["power"]:
                if initial_info["rgb"] and initial_info["rgb"][0] is not None:
                    await bulb.turn_on(
                        PilotBuilder(
                            rgb=initial_info["rgb"],
                            brightness=initial_info["brightness"],
                        )
                    )
                elif initial_info["colortemp"]:
                    await bulb.turn_on(
                        PilotBuilder(
                            colortemp=initial_info["colortemp"],
                            brightness=initial_info["brightness"],
                        )
                    )
                elif initial_info["scene_id"]:
                    await bulb.turn_on(
                        PilotBuilder(
                            scene=initial_info["scene_id"],
                            brightness=initial_info["brightness"],
                        )
                    )
                else:
                    await bulb.turn_on(
                        PilotBuilder(brightness=initial_info["brightness"])
                    )
            else:
                await bulb.turn_off()
        finally:
            await bulb.async_close()
