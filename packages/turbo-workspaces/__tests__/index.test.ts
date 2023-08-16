import path from "path";
import { setupTestFixtures } from "turbo-test-utils";
import { getWorkspaceDetails, convertMonorepo } from "../src";
import { generateConvertMatrix } from "./test-utils";
import execa from "execa";

jest.mock("execa", () => jest.fn());

describe("Node entrypoint", () => {
  const { useFixture } = setupTestFixtures({
    directory: path.join(__dirname, "../"),
    test: "npm",
  });

  test.each(generateConvertMatrix())(
    "detects project using %s workspaces and converts to %s workspaces | interactive=%s dry=%s skipInstall=%s",
    async (from, to, interactive, dry, skipInstall) => {
      const { root } = useFixture({ fixture: `../${from}/basic` });
      // read
      const details = await getWorkspaceDetails({ root });
      expect(details.packageManager).toBe(from);

      // convert
      const convert = () =>
        convertMonorepo({
          root,
          to,
          options: { interactive, dry, skipInstall },
        });

      if (from === to) {
        await expect(convert()).rejects.toThrowError(
          "You are already using this package manager"
        );
      } else {
        await expect(convert()).resolves.toBeUndefined();
        // read again
        const convertedDetails = await getWorkspaceDetails({
          root,
        });
        if (dry) {
          expect(convertedDetails.packageManager).toBe(from);
        } else {
          if (!skipInstall) {
            expect(execa).toHaveBeenCalled();
          }
          expect(convertedDetails.packageManager).toBe(to);
        }
      }
    }
  );
});
