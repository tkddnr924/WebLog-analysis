// Contains render exceptions per screen: header and notice survive, and the user can retry or move on.
import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  /** Screen name shown to the user. */
  label: string;
  /** Changing this clears the error state and re-renders. */
  resetKey?: string | number;
  children: ReactNode;
}

interface State {
  message: string | null;
  stack: string | null;
  resetKey: string | number | undefined;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { message: null, stack: null, resetKey: props.resetKey };
  }

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return {
      message: error instanceof Error ? error.message : String(error),
      stack: error instanceof Error ? (error.stack ?? null) : null,
    };
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    if (props.resetKey === state.resetKey) return null;
    return { message: null, stack: null, resetKey: props.resetKey };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Code location only, never log content.
    console.error(`${this.props.label} 렌더 실패`, error, info.componentStack);
  }

  render() {
    if (this.state.message === null) return this.props.children;
    return (
      <section className="render-error" role="alert">
        <h2>{this.props.label}을 표시하지 못했습니다</h2>
        <p>화면을 그리는 중 오류가 났습니다. 저장된 데이터는 그대로이며, 다시 시도하거나 다른 화면으로 이동할 수 있습니다.</p>
        <p className="render-error-msg mono">{this.state.message}</p>
        {this.state.stack !== null && (
          <details>
            <summary>자세히</summary>
            <pre className="mono small">{this.state.stack}</pre>
          </details>
        )}
        <div className="row">
          <button onClick={() => this.setState({ message: null, stack: null })}>다시 시도</button>
        </div>
      </section>
    );
  }
}
