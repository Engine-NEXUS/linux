import { useEffect, useRef } from "react";
import lottie, { AnimationItem } from "lottie-web";

export function LoadingAnimation() {
  const containerRef = useRef<HTMLDivElement>(null);
  const animRef = useRef<AnimationItem | null>(null);

  useEffect(() => {
    let destroyed = false;

    fetch(`${import.meta.env.BASE_URL}loading.json`)
      .then((res) => res.json())
      .then((data) => {
        if (destroyed || !containerRef.current) return;

        const anim = lottie.loadAnimation({
          container: containerRef.current,
          renderer: "svg",
          loop: true,
          autoplay: true,
          animationData: data,
        });
        animRef.current = anim;
      })
      .catch((err) => console.error("LoadingAnimation: failed to load lottie:", err));

    return () => {
      destroyed = true;
      if (animRef.current) {
        animRef.current.destroy();
        animRef.current = null;
      }
    };
  }, []);

  return (
    <div className="loading-animation-container" ref={containerRef} style={{ width: 80, height: 80, margin: '0 auto' }} />
  );
}
