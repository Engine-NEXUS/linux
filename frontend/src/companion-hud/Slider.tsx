import NumberFlow from "@number-flow/react";
import * as RadixSlider from "@radix-ui/react-slider";
import clsx from "clsx";

export interface SliderProps extends RadixSlider.SliderProps {
  value?: number[];
  onValueChange?: (value: number[]) => void;
  min?: number;
  max?: number;
  step?: number;
  className?: string;
}

export function Slider({
  value,
  className,
  onValueChange,
  min,
  max,
  step,
  ...props
}: SliderProps) {
  return (
    <RadixSlider.Root
      {...props}
      value={value}
      onValueChange={onValueChange}
      min={min}
      max={max}
      step={step}
      className={clsx(
        className,
        "hud-slider-root"
      )}
    >
      <RadixSlider.Track className="hud-slider-track">
        <RadixSlider.Range className="hud-slider-range" />
      </RadixSlider.Track>
      <RadixSlider.Thumb
        className="hud-slider-thumb"
        aria-label="Size"
      >
        {value?.[0] != null && (
          <div className="hud-slider-tooltip">
            {/* Plan 04 §2: tooltip shows the 0–100 slider value; px lives
                in the background (Rust drafts + desktop badge). */}
            <NumberFlow
              willChange
              value={value[0]}
              isolate
              opacityTiming={{
                duration: 250,
                easing: "ease-out",
              }}
              transformTiming={{
                easing: `linear(0, 0.0033 0.8%, 0.0263 2.39%, 0.0896 4.77%, 0.4676 15.12%, 0.5688, 0.6553, 0.7274, 0.7862, 0.8336 31.04%, 0.8793, 0.9132 38.99%, 0.9421 43.77%, 0.9642 49.34%, 0.9796 55.71%, 0.9893 62.87%, 0.9952 71.62%, 0.9983 82.76%, 0.9996 99.47%)`,
                duration: 500,
              }}
            />
          </div>
        )}
      </RadixSlider.Thumb>
    </RadixSlider.Root>
  );
}

export default Slider;
