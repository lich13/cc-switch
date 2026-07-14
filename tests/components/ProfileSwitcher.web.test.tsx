import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { isWebRuntime } from "@/lib/runtime";

const useProfilesQueryMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/runtime", () => ({
  isWebRuntime: vi.fn(),
}));

vi.mock("@/lib/query/profiles", () => ({
  useProfilesQuery: useProfilesQueryMock,
  useApplyProfileMutation: () => ({ mutate: vi.fn() }),
  useClearProfileMutation: () => ({ mutate: vi.fn() }),
  useCreateProfileMutation: () => ({
    isPending: false,
    mutate: vi.fn(),
  }),
}));

vi.mock("@/components/profiles/ProfileManageDialog", () => ({
  ProfileManageDialog: () => null,
}));

describe("ProfileSwitcher runtime boundary", () => {
  beforeEach(() => {
    vi.mocked(isWebRuntime).mockReset();
    useProfilesQueryMock.mockReset();
    useProfilesQueryMock.mockReturnValue({
      data: {
        profiles: [],
        currentIds: {
          claude: null,
          claudeDesktop: null,
          codex: null,
        },
      },
    });
  });

  it("does not mount the profile query or entry in web runtime", () => {
    vi.mocked(isWebRuntime).mockReturnValue(true);

    render(<ProfileSwitcher activeApp="claude" />);

    expect(useProfilesQueryMock).not.toHaveBeenCalled();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  });

  it("keeps the profile query and entry in desktop runtime", () => {
    vi.mocked(isWebRuntime).mockReturnValue(false);

    render(<ProfileSwitcher activeApp="claude" />);

    expect(useProfilesQueryMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("combobox")).toBeInTheDocument();
  });
});
