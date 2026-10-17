package org.tapirscan.android;

import java.util.Objects;

/** Enclosing integer pixel bounds of a polygon. */
public final class Rect {
    private final int left;
    private final int top;
    private final int width;
    private final int height;

    Rect(int left, int top, int width, int height) {
        this.left = left;
        this.top = top;
        this.width = width;
        this.height = height;
    }

    public int left() {
        return left;
    }

    public int top() {
        return top;
    }

    public int width() {
        return width;
    }

    public int height() {
        return height;
    }

    @Override
    public boolean equals(Object other) {
        if (!(other instanceof Rect)) return false;
        Rect r = (Rect) other;
        return left == r.left && top == r.top && width == r.width && height == r.height;
    }

    @Override
    public int hashCode() {
        return Objects.hash(left, top, width, height);
    }

    @Override
    public String toString() {
        return "Rect[left=" + left + ", top=" + top + ", width=" + width + ", height=" + height + "]";
    }
}
