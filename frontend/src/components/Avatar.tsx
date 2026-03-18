import styles from "./Avatar.module.css";
import type { User } from "../api/types";
import { getAvatarUrl } from "../api/profile";

interface AvatarProps {
  user: User;
  size?: "sm" | "md" | "lg";
}

export function Avatar({ user, size = "md" }: AvatarProps) {
  const sizeClass = styles[size];

  // Generate consistent color from user ID
  const getColorFromId = (id: string) => {
    let hash = 0;
    for (let i = 0; i < id.length; i++) {
      hash = id.charCodeAt(i) + ((hash << 5) - hash);
    }
    const hue = Math.abs(hash % 360);
    return `hsl(${hue}, 65%, 50%)`;
  };

  const initial = (user.display_name || user.name).charAt(0).toUpperCase();
  const bgColor = getColorFromId(user.id);

  if (user.avatar_path) {
    return (
      <img
        src={getAvatarUrl(user.id)}
        alt={user.display_name || user.name}
        className={`${styles.avatar} ${sizeClass}`}
        onError={(e) => {
          // Fallback to placeholder if image fails to load
          const target = e.target as HTMLImageElement;
          target.style.display = "none";
          if (target.nextSibling) {
            (target.nextSibling as HTMLElement).style.display = "flex";
          }
        }}
      />
    );
  }

  return (
    <div
      className={`${styles.placeholder} ${sizeClass}`}
      style={{ backgroundColor: bgColor }}
    >
      {initial}
    </div>
  );
}
