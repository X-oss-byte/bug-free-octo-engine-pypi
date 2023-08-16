import * as React from "react";
import type { Issue } from "@vercel/turbopack-runtime/types/protocol";

import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogHeader,
} from "../components/Dialog";
import { Overlay } from "../components/Overlay";
import { Terminal } from "../components/Terminal";
import { noop as css } from "../helpers/noop-template";

export type BuildErrorProps = { issue: Issue };

export function BuildError({ issue }: BuildErrorProps) {
  const noop = React.useCallback(() => {}, []);
  return (
    <Overlay fixed>
      <Dialog
        aria-labelledby="nextjs__container_build_error_label"
        aria-describedby="nextjs__container_build_error_desc"
        onClose={noop}
      >
        <DialogContent>
          <DialogHeader className="nextjs-container-build-error-header">
            <h4 id="nextjs__container_build_error_label">
              Turbopack failed to compile
            </h4>
          </DialogHeader>
          <DialogBody className="nextjs-container-build-error-body">
            <Terminal content={issue.formatted} />
            <footer>
              <p id="nextjs__container_build_error_desc">
                <small>
                  This error occurred during the build process and can only be
                  dismissed by fixing the error.
                </small>
              </p>
            </footer>
          </DialogBody>
        </DialogContent>
      </Dialog>
    </Overlay>
  );
}

export const styles = css`
  .nextjs-container-build-error-header > h4 {
    line-height: 1.5;
    margin: 0;
    padding: 0;
  }

  .nextjs-container-build-error-body footer {
    margin-top: var(--size-gap);
  }
  .nextjs-container-build-error-body footer p {
    margin: 0;
  }

  .nextjs-container-build-error-body small {
    color: #757575;
  }
`;
